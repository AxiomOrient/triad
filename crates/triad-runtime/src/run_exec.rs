use camino::{Utf8Path, Utf8PathBuf};
use triad_config::{AgentBackend, CanonicalTriadConfig};
use triad_core::{ClaimId, RunClaimReport, RunClaimRequest, TriadError};

use crate::agent_runtime::workspace_stage::WorkspaceChange;
use crate::agent_runtime::{
    AgentRuntimeAdapter, BackendCapabilityProbe, ClaudeAdapter, CodexAdapter, GeminiAdapter,
    PreparedProcessInvocation, ProcessRunner, RawInvocationOutput, WorkspaceChangeKind,
    WorkspaceStage, probe_backend_capabilities, stage_workspace,
    validate_run_request_against_probe,
};
use crate::run_result::{parse_run_claim_response, prompt_fingerprint, work_runtime_metadata};
use crate::work_contract::build_adapter_run_request;
use crate::{LocalTriad, WorkGuardrails, WorkToolUse};

pub(crate) fn run_claim_with_backend_adapter<P: ProcessRunner>(
    triad: &LocalTriad,
    req: RunClaimRequest,
    process_runner: &P,
) -> Result<RunClaimReport, TriadError> {
    let probe = probe_backend_capabilities(&triad.config)?;
    validate_run_request_against_probe(&triad.config, &req, &probe)?;
    let adapter = resolve_agent_runtime_adapter(&triad.config)?;
    run_claim_with_adapter(triad, req, adapter.as_ref(), process_runner, &probe)
}

pub(crate) fn run_claim_with_adapter<P: ProcessRunner>(
    triad: &LocalTriad,
    req: RunClaimRequest,
    adapter: &dyn AgentRuntimeAdapter,
    process_runner: &P,
    probe: &BackendCapabilityProbe,
) -> Result<RunClaimReport, TriadError> {
    let run_id = triad.next_run_id()?;
    let workspace_stage = stage_workspace(
        &triad.config.repo_root,
        &triad.config.paths.state_dir,
        &run_id,
    )?;
    let adapter_request = build_adapter_run_request(triad, &req, workspace_stage.workspace_root())?;
    validate_adapter_request(triad, &req, adapter, probe)?;
    let invocation = adapter.prepare_invocation(&adapter_request)?;
    let invocation_output = execute_adapter_invocation(&invocation, process_runner)?;
    let completion = adapter.complete(invocation_output.clone())?;
    let report = parse_run_claim_response(&completion.assistant_text, &req.claim_id, run_id)?;
    enforce_live_work_guardrails(triad, &req.claim_id, &report)?;
    apply_workspace_copy_back(triad, &req.claim_id, &workspace_stage, &report)?;
    let prompt_fingerprint = prompt_fingerprint(&adapter_request.prompt_text);
    let runtime_metadata = work_runtime_metadata(triad, &req, &invocation, &invocation_output);
    triad.store_run_record(&report, &prompt_fingerprint, &runtime_metadata)?;
    Ok(report)
}

pub(crate) fn resolve_agent_runtime_adapter(
    config: &CanonicalTriadConfig,
) -> Result<Box<dyn AgentRuntimeAdapter>, TriadError> {
    match config.agent.backend {
        AgentBackend::Codex => Ok(Box::new(CodexAdapter)),
        AgentBackend::Claude => Ok(Box::new(ClaudeAdapter)),
        AgentBackend::Gemini => Ok(Box::new(GeminiAdapter)),
    }
}

pub(crate) fn validate_adapter_request(
    triad: &LocalTriad,
    req: &RunClaimRequest,
    adapter: &dyn AgentRuntimeAdapter,
    probe: &BackendCapabilityProbe,
) -> Result<(), TriadError> {
    if adapter.backend() != triad.config.agent.backend {
        return Err(TriadError::InvalidState(format!(
            "adapter/backend mismatch: adapter={}, config={}",
            adapter.backend().as_str(),
            triad.config.agent.backend.as_str()
        )));
    }

    validate_run_request_against_probe(&triad.config, req, probe)
}

pub(crate) fn execute_adapter_invocation<P: ProcessRunner>(
    invocation: &PreparedProcessInvocation,
    process_runner: &P,
) -> Result<RawInvocationOutput, TriadError> {
    let output = process_runner.run(invocation)?;
    Ok(RawInvocationOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
    })
}

pub(crate) fn enforce_live_work_guardrails(
    triad: &LocalTriad,
    claim_id: &ClaimId,
    report: &RunClaimReport,
) -> Result<(), TriadError> {
    let guardrails = triad.work_guardrails(claim_id, &default_live_work_write_roots(triad))?;

    for action in &report.blocked_actions {
        enforce_reported_blocked_action(&guardrails, action)?;
    }

    for path in &report.changed_paths {
        let path = Utf8PathBuf::from(path.as_str());
        if should_ignore_reported_changed_path(triad, &path) {
            continue;
        }
        guardrails.check(&WorkToolUse::WritePath { path })?;
    }

    Ok(())
}

pub(crate) fn apply_workspace_copy_back(
    triad: &LocalTriad,
    claim_id: &ClaimId,
    workspace_stage: &WorkspaceStage,
    report: &RunClaimReport,
) -> Result<(), TriadError> {
    let changes = workspace_stage
        .changed_paths()?
        .into_iter()
        .filter(|change| !should_ignore_workspace_change(triad, change))
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return Ok(());
    }

    match triad.config.agent.sandbox_policy.as_str() {
        "read-only" => {
            return Err(TriadError::RuntimeBlocked(
                "copy-back blocked by read-only sandbox policy".to_string(),
            ));
        }
        "workspace-write" => {}
        "danger-full-access" => {
            return Err(TriadError::config_field(
                "agent.sandbox_policy",
                "danger-full-access is unsupported for staged one-shot work",
            ));
        }
        other => {
            return Err(TriadError::config_field(
                "agent.sandbox_policy",
                &format!("unknown sandbox policy: {other}"),
            ));
        }
    }

    let guardrails = triad.work_guardrails(claim_id, &default_live_work_write_roots(triad))?;
    let reported = report
        .changed_paths
        .iter()
        .map(|path| path.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    for change in &changes {
        if !reported.contains(change.path.as_str()) {
            return Err(TriadError::RuntimeBlocked(format!(
                "workspace diff missing from reported changed_paths: {}",
                change.path
            )));
        }

        match change.kind {
            WorkspaceChangeKind::Added | WorkspaceChangeKind::Modified => {
                guardrails.check(&WorkToolUse::WritePath {
                    path: change.path.clone(),
                })?;
            }
            WorkspaceChangeKind::Removed => {
                guardrails.check(&WorkToolUse::RemovePath {
                    path: change.path.clone(),
                    recursive: false,
                })?;
            }
        }
    }

    workspace_stage.apply_changes(&changes)
}

fn is_internal_workspace_artifact(path: &Utf8Path) -> bool {
    path == Utf8Path::new(".triad") || path.starts_with(".triad/")
}

fn should_ignore_workspace_change(triad: &LocalTriad, change: &WorkspaceChange) -> bool {
    if is_internal_workspace_artifact(&change.path) {
        return true;
    }

    should_ignore_new_root_cargo_lock(triad, &change.path, Some(change.kind))
}

fn should_ignore_reported_changed_path(triad: &LocalTriad, path: &Utf8Path) -> bool {
    is_internal_workspace_artifact(path) || should_ignore_new_root_cargo_lock(triad, path, None)
}

fn should_ignore_new_root_cargo_lock(
    triad: &LocalTriad,
    path: &Utf8Path,
    change_kind: Option<WorkspaceChangeKind>,
) -> bool {
    if path != Utf8Path::new("Cargo.lock") {
        return false;
    }

    if matches!(change_kind, Some(kind) if kind != WorkspaceChangeKind::Added) {
        return false;
    }

    !triad.config.repo_root.join("Cargo.lock").exists()
}

fn default_live_work_write_roots(triad: &LocalTriad) -> Vec<Utf8PathBuf> {
    vec![
        triad.config.repo_root.join("src"),
        triad.config.repo_root.join("tests"),
        triad.config.repo_root.join("crates"),
        triad.config.paths.state_dir.join("tmp"),
    ]
}

fn enforce_reported_blocked_action(
    guardrails: &WorkGuardrails,
    action: &str,
) -> Result<(), TriadError> {
    let normalized = action.trim().to_ascii_lowercase();

    if normalized.starts_with("git commit") {
        return guardrails.check(&WorkToolUse::Exec {
            program: "git".to_string(),
            args: vec!["commit".to_string()],
        });
    }

    if normalized.starts_with("git push") {
        return guardrails.check(&WorkToolUse::Exec {
            program: "git".to_string(),
            args: vec!["push".to_string()],
        });
    }

    if let Some(remove) = parse_reported_recursive_remove(action) {
        return guardrails.check(&remove);
    }

    Err(TriadError::RuntimeBlocked(format!(
        "work reported blocked action: {}",
        action.trim()
    )))
}

fn parse_reported_recursive_remove(action: &str) -> Option<WorkToolUse> {
    let tokens = action.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 3 || tokens.first().copied() != Some("rm") {
        return None;
    }

    let recursive = tokens[1..tokens.len() - 1]
        .iter()
        .any(|token| token.starts_with('-') && token.contains('r'));
    if !recursive {
        return None;
    }

    let path = tokens.last()?.trim();
    if path.is_empty() {
        return None;
    }

    Some(WorkToolUse::RemovePath {
        path: Utf8PathBuf::from(path),
        recursive: true,
    })
}
