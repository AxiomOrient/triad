use std::{
    cell::RefCell,
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use camino::Utf8PathBuf;
use triad_config::{
    AgentBackend, AgentConfig, CanonicalPathConfig, CanonicalTriadConfig, ClaudeBackendConfig,
    CodexBackendConfig, GuardrailConfig, VerifyConfig,
};
use triad_core::error::TriadErrorKind;
use triad_core::{
    Claim, ClaimId, DriftStatus, Evidence, EvidenceId, EvidenceKind, NextAction, PatchDraft,
    PatchId, PatchState, ReasoningLevel, RunClaimReport, RunClaimRequest, RunId, TriadApi,
    TriadError, Verdict, VerifyLayer, VerifyReport, VerifyRequest,
};

use super::{
    LocalTriad, ProcessCommandRunner, VerifyCommandExecution, VerifyCommandPlan,
    VerifyCommandRunner, WorkToolUse, append_evidence, apply_patch_with_runner,
    deterministic_mismatch_for_claim, execute_verify_commands_with_runner, next_evidence_id,
    read_evidence, run_claim_with_backend_adapter, verify_claim_with_runner,
};
use crate::agent_runtime::process_runner::ProcessExecutionOutput;
use crate::agent_runtime::{
    ApprovalPolicy, ProcessRunner, PromptAttachment, ReasoningEffort, SandboxPolicy, SandboxPreset,
};
use crate::claims::{canonical_claim_revision_bytes, claim_revision_number, parse_claim_file};
use crate::run_result::prompt_fingerprint;
use crate::scaffold::DEFAULT_SCHEMA_FILES;

struct FakeCommandRunner {
    exit_codes: BTreeMap<String, i32>,
    seen: RefCell<Vec<VerifyCommandPlan>>,
}

impl FakeCommandRunner {
    fn new(exit_codes: BTreeMap<String, i32>) -> Self {
        Self {
            exit_codes,
            seen: RefCell::new(Vec::new()),
        }
    }

    fn seen(&self) -> Vec<VerifyCommandPlan> {
        self.seen.borrow().clone()
    }
}

impl VerifyCommandRunner for FakeCommandRunner {
    fn run(&self, plan: &VerifyCommandPlan) -> Result<i32, TriadError> {
        self.seen.borrow_mut().push(plan.clone());
        Ok(*self.exit_codes.get(&plan.command).unwrap_or(&0))
    }
}

#[derive(Clone)]
struct FakeProcessWrite {
    path: String,
    content: String,
}

struct FakeProcessRunner {
    calls: RefCell<Vec<crate::agent_runtime::PreparedProcessInvocation>>,
    stdout: String,
    stderr: String,
    exit_code: i32,
    workspace_writes: Vec<FakeProcessWrite>,
}

impl Default for FakeProcessRunner {
    fn default() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            workspace_writes: Vec::new(),
        }
    }
}

impl FakeProcessRunner {
    fn success(assistant_text: impl Into<String>, writes: Vec<(String, String)>) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            stdout: assistant_text.into(),
            stderr: String::new(),
            exit_code: 0,
            workspace_writes: writes
                .into_iter()
                .map(|(path, content)| FakeProcessWrite { path, content })
                .collect(),
        }
    }

    fn codex_success(assistant_text: impl Into<String>, writes: Vec<(String, String)>) -> Self {
        Self::success(assistant_text, writes)
    }

    fn failure(stdout: impl Into<String>, stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit_code,
            workspace_writes: Vec::new(),
        }
    }

    fn calls(&self) -> Vec<crate::agent_runtime::PreparedProcessInvocation> {
        self.calls.borrow().clone()
    }
}

impl ProcessRunner for FakeProcessRunner {
    fn run(
        &self,
        invocation: &crate::agent_runtime::PreparedProcessInvocation,
    ) -> Result<ProcessExecutionOutput, TriadError> {
        self.calls.borrow_mut().push(invocation.clone());
        match &invocation.capture_mode {
            crate::agent_runtime::ProcessCaptureMode::Stdout => {}
            crate::agent_runtime::ProcessCaptureMode::OutputFile { path } => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent.as_std_path()).map_err(|err| {
                        TriadError::Io(format!(
                            "failed to create fake process output dir {}: {err}",
                            parent
                        ))
                    })?;
                }
                fs::write(path.as_std_path(), &self.stdout).map_err(|err| {
                    TriadError::Io(format!(
                        "failed to write fake process output file {}: {err}",
                        path
                    ))
                })?;
            }
        }
        for write in &self.workspace_writes {
            let full_path = invocation.cwd.join(&write.path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent.as_std_path()).map_err(|err| {
                    TriadError::Io(format!(
                        "failed to create fake process workspace dir {}: {err}",
                        parent
                    ))
                })?;
            }
            fs::write(full_path.as_std_path(), &write.content).map_err(|err| {
                TriadError::Io(format!(
                    "failed to write fake process workspace file {}: {err}",
                    full_path
                ))
            })?;
        }
        Ok(ProcessExecutionOutput {
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
            exit_code: self.exit_code,
        })
    }
}

#[test]
fn session_config_from_triad_maps_agent_settings_to_local_runtime_types() {
    let temp = TestDir::new("session-config-from-triad");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    let profile = triad.run_profile().expect("run profile should build");
    assert_eq!(profile.model.as_deref(), Some("gpt-5-codex"));
    assert_eq!(profile.effort, ReasoningEffort::Medium);
    assert_eq!(profile.approval_policy, ApprovalPolicy::Never);
    assert_eq!(profile.timeout, std::time::Duration::from_secs(600));
    assert_eq!(
        profile.sandbox_policy,
        SandboxPolicy::Preset(SandboxPreset::WorkspaceWrite {
            writable_roots: vec![repo_root.display().to_string()],
            network_access: false,
        })
    );

    let session = triad.session_config().expect("session config should build");
    assert_eq!(session.cwd, repo_root.display().to_string());
    assert_eq!(session.model.as_deref(), Some("gpt-5-codex"));
    assert_eq!(session.effort, ReasoningEffort::Medium);
    assert_eq!(session.approval_policy, ApprovalPolicy::Never);
    assert_eq!(session.timeout, std::time::Duration::from_secs(600));
    assert_eq!(session.sandbox_policy, profile.sandbox_policy);
}

#[test]
fn session_config_from_triad_rejects_invalid_agent_settings() {
    let temp = TestDir::new("session-config-invalid");
    let repo_root = temp.path();

    let mut invalid_effort = test_config(repo_root);
    invalid_effort.agent.effort = "ultra".into();
    let triad = LocalTriad::new(invalid_effort);
    let error = triad.run_profile().expect_err("invalid effort should fail");
    assert_eq!(
        error.to_string(),
        "config error: invalid config agent.effort: unknown reasoning effort: ultra"
    );

    let mut invalid_sandbox = test_config(repo_root);
    invalid_sandbox.agent.sandbox_policy = "wild-west".into();
    let triad = LocalTriad::new(invalid_sandbox);
    let error = triad
        .session_config()
        .expect_err("invalid sandbox should fail");
    assert_eq!(
        error.to_string(),
        "config error: invalid config agent.sandbox_policy: unknown sandbox policy: wild-west"
    );

    let mut invalid_timeout = test_config(repo_root);
    invalid_timeout.agent.timeout_seconds = 0;
    let triad = LocalTriad::new(invalid_timeout);
    let error = triad.run_profile().expect_err("zero timeout should fail");
    assert_eq!(
        error.to_string(),
        "config error: invalid config agent.timeout_seconds: must be > 0"
    );
}

#[test]
fn adapter_contract_run_claim_uses_backend_adapter_seam_for_codex() {
    let temp = TestDir::new("adapter-contract-run-claim-codex");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("crates/auth/src")).expect("crate dir should exist");
    fs::write(
        repo_root.join("crates/auth/src/lib.rs"),
        "pub fn login() {}\n",
    )
    .expect("source file should be written");
    let process_runner = FakeProcessRunner::codex_success(
        r#"{"schema_version":1,"ok":true,"command":"run","data":{"claim_id":"REQ-auth-001","summary":"implemented login","changed_paths":["crates/auth/src/lib.rs"],"suggested_test_selectors":["auth_login"],"blocked_actions":[],"needs_patch":false}}"#,
        vec![(
            "crates/auth/src/lib.rs".to_string(),
            "pub fn login() -> bool { true }\n".to_string(),
        )],
    );

    let report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: Some("gpt-5-mini".to_string()),
            effort: Some(ReasoningLevel::High),
        },
        &process_runner,
    )
    .expect("codex bridge path should succeed");

    assert_eq!(report.summary, "implemented login");
    assert_eq!(report.changed_paths, vec!["crates/auth/src/lib.rs"]);
    assert_eq!(process_runner.calls().len(), 1);
    assert_eq!(process_runner.calls()[0].program, "codex");
    assert_eq!(
        fs::read_to_string(repo_root.join("crates/auth/src/lib.rs"))
            .expect("source file should read"),
        "pub fn login() -> bool { true }\n"
    );
}

#[test]
fn adapter_contract_run_claim_uses_backend_adapter_seam_for_claude() {
    let temp = TestDir::new("adapter-contract-run-claim-claude");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.agent.backend = AgentBackend::Claude;
    config.agent.model = "claude-sonnet-4".to_string();
    config.agent.codex = None;
    config.agent.claude = Some(ClaudeBackendConfig {
        permission_mode: Some("acceptEdits".to_string()),
    });
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");
    let process_runner = FakeProcessRunner::success(
        r#"[{"type":"assistant","message":"ignored"},{"type":"result","result":"{\"schema_version\":1,\"ok\":true,\"command\":\"run\",\"data\":{\"claim_id\":\"REQ-auth-001\",\"summary\":\"implemented login with claude\",\"changed_paths\":[\"src/auth.rs\"],\"suggested_test_selectors\":[\"auth_login\"],\"blocked_actions\":[],\"needs_patch\":false},\"diagnostics\":[]}"}]"#,
        vec![(
            "src/auth.rs".to_string(),
            "pub fn login() -> bool { true }\n".to_string(),
        )],
    );

    let report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        },
        &process_runner,
    )
    .expect("claude process path should succeed");

    assert_eq!(report.summary, "implemented login with claude");
    assert_eq!(report.changed_paths, vec!["src/auth.rs"]);
    assert_eq!(process_runner.calls().len(), 1);
    assert_eq!(process_runner.calls()[0].program, "claude");
    assert!(
        process_runner.calls()[0]
            .args
            .windows(2)
            .any(|window| window == ["--output-format", "json"])
    );
    assert!(
        process_runner.calls()[0]
            .args
            .windows(2)
            .any(|window| window == ["--model", "claude-sonnet-4"])
    );
    assert!(
        process_runner.calls()[0]
            .args
            .windows(2)
            .any(|window| window == ["--effort", "medium"])
    );
    assert!(
        process_runner.calls()[0]
            .args
            .windows(2)
            .any(|window| window == ["--permission-mode", "acceptEdits"])
    );
    assert_eq!(
        fs::read_to_string(repo_root.join("src/auth.rs")).expect("source file should read"),
        "pub fn login() -> bool { true }\n"
    );
}

#[test]
fn adapter_contract_run_claim_uses_backend_adapter_seam_for_gemini() {
    let temp = TestDir::new("adapter-contract-run-claim-gemini");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.agent.backend = AgentBackend::Gemini;
    config.agent.model = "gemini-2.5-pro".to_string();
    config.agent.codex = None;
    config.agent.gemini = Some(Default::default());
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");
    let process_runner = FakeProcessRunner::success(
        r#"{"response":"```json\n{\"schema_version\":1,\"ok\":true,\"command\":\"run\",\"data\":{\"claim_id\":\"REQ-auth-001\",\"summary\":\"implemented login with gemini\",\"changed_paths\":[\"src/auth.rs\"],\"suggested_test_selectors\":[\"auth_login\"],\"blocked_actions\":[],\"needs_patch\":false}}\n```","stats":{"models":{"gemini-2.5-pro":{"api":{"totalRequests":1}}}}}"#,
        vec![(
            "src/auth.rs".to_string(),
            "pub fn login() -> bool { true }\n".to_string(),
        )],
    );

    let report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        },
        &process_runner,
    )
    .expect("gemini process path should succeed");

    assert_eq!(report.summary, "implemented login with gemini");
    assert_eq!(report.changed_paths, vec!["src/auth.rs"]);
    assert_eq!(process_runner.calls().len(), 1);
    assert_eq!(process_runner.calls()[0].program, "gemini");
    assert!(
        process_runner.calls()[0]
            .args
            .windows(2)
            .any(|window| window == ["--output-format", "json"])
    );
    assert!(
        process_runner.calls()[0]
            .args
            .windows(2)
            .any(|window| window == ["--model", "gemini-2.5-pro"])
    );
    assert!(
        process_runner.calls()[0]
            .args
            .windows(2)
            .any(|window| window == ["--approval-mode", "yolo"])
    );
    assert!(
        process_runner.calls()[0]
            .args
            .contains(&"--sandbox".to_string())
    );
    assert_eq!(
        fs::read_to_string(repo_root.join("src/auth.rs")).expect("source file should read"),
        "pub fn login() -> bool { true }\n"
    );
}

#[test]
fn backend_probe_run_claim_rejects_gemini_effort_override_before_invocation() {
    let temp = TestDir::new("backend-probe-run-claim-gemini");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.agent.backend = AgentBackend::Gemini;
    config.agent.codex = None;
    config.agent.gemini = Some(Default::default());
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    let process_runner = FakeProcessRunner::default();

    let error = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: Some(ReasoningLevel::High),
        },
        &process_runner,
    )
    .expect_err("gemini effort override should be rejected");

    assert_eq!(
        error.to_string(),
        "config error: invalid config agent.effort: gemini backend does not support effort override"
    );
    assert!(process_runner.calls().is_empty());
}

#[test]
fn single_claim_work_uses_one_process_invocation_and_returns_structured_report() {
    let temp = TestDir::new("single-claim-work");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    let process_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "updated login flow",
"changed_paths": ["crates/triad-runtime/src/lib.rs", "tests/login.rs"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );

    let report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: Some("gpt-5-mini".to_string()),
            effort: Some(ReasoningLevel::High),
        },
        &process_runner,
    )
    .expect("run claim should succeed");

    assert_eq!(process_runner.calls().len(), 1);
    let invocation = &process_runner.calls()[0];
    assert_eq!(invocation.program, "codex");
    assert_eq!(invocation.model, "gpt-5-mini");
    assert_eq!(invocation.effort, "high");
    assert_eq!(
        invocation.stdin.as_deref(),
        Some(
            "You are implementing exactly one triad claim.\n\nSelected claim: REQ-auth-001\nUse only the attached project rules and the attached selected claim as repository context.\nDo not infer or load unrelated claims or unrelated docs.\n\nForbidden actions:\n- Write to spec/claims/** during work.\n- Run git commit or git push.\n- Remove files recursively outside an explicitly approved temporary workspace.\n- Modify files unrelated to the selected claim.\n\nOutput requirements:\n- Return JSON only, matching the configured output schema.\n- changed_paths must list every modified repo file. Exclude ignored derived artifacts such as target/** and a newly generated Cargo.lock.\n- Set claim_id to REQ-auth-001."
        )
    );
    let staged_schema: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            repo_root.join(".triad/tmp/workspaces/RUN-000001/.triad/runtime-agent.run.schema.json"),
        )
        .expect("staged work schema should be written"),
    )
    .expect("staged work schema should parse");
    assert_eq!(staged_schema["type"], "object");
    assert_eq!(staged_schema["properties"]["command"]["const"], "run");
    assert!(staged_schema.get("allOf").is_none());
    assert_eq!(report.run_id.as_str(), "RUN-000001");
    assert_eq!(report.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(report.summary, "updated login flow");
    assert_eq!(
        report.changed_paths,
        vec![
            "crates/triad-runtime/src/lib.rs".to_string(),
            "tests/login.rs".to_string()
        ]
    );
    assert_eq!(
        report.suggested_test_selectors,
        vec!["auth::login_success".to_string()]
    );
    assert!(report.blocked_actions.is_empty());
    assert!(!report.needs_patch);
}

#[test]
fn single_claim_work_rejects_agent_output_for_another_claim() {
    let temp = TestDir::new("single-claim-work-mismatch");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    let process_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-002",
"summary": "wrong claim",
"changed_paths": [],
"suggested_test_selectors": [],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );

    let error = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        },
        &process_runner,
    )
    .expect_err("mismatched claim output should fail");

    assert_eq!(process_runner.calls().len(), 1);
    assert_eq!(
        error.to_string(),
        "invalid state: work response claim_id mismatch: expected REQ-auth-001, got REQ-auth-002"
    );
}

#[test]
fn work_persists_run_record_with_prompt_fingerprint_and_runtime_metadata() {
    let temp = TestDir::new("work-persists-run");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    let process_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "updated login flow",
"changed_paths": ["crates/triad-runtime/src/lib.rs"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": true
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );

    let report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: Some("gpt-5-mini".to_string()),
            effort: Some(ReasoningLevel::High),
        },
        &process_runner,
    )
    .expect("run claim should succeed");

    let record = triad
        .read_run_record(&report.run_id)
        .expect("run record should read");

    assert_eq!(record.run_id, report.run_id);
    assert_eq!(record.changed_paths, report.changed_paths);
    assert_eq!(
        record.suggested_test_selectors,
        report.suggested_test_selectors
    );
    assert_eq!(record.blocked_actions, report.blocked_actions);
    assert_eq!(
        record.prompt_fingerprint,
        prompt_fingerprint(
            process_runner.calls()[0]
                .stdin
                .as_deref()
                .expect("prompt should exist")
        )
    );
    assert_eq!(record.runtime_metadata["model"], "gpt-5-mini");
    assert_eq!(record.runtime_metadata["effort"], "high");
    assert_eq!(record.runtime_metadata["approval_policy"], "never");
    assert_eq!(record.runtime_metadata["sandbox_policy"], "workspace-write");
    assert_eq!(record.runtime_metadata["program"], "codex");
    assert_eq!(record.runtime_metadata["exit_code"], "0");
    assert_eq!(record.runtime_metadata["dry_run"], "false");
    assert_eq!(
        triad
            .next_run_id()
            .expect("next run id should advance after persisted work")
            .as_str(),
        "RUN-000002"
    );
}

#[test]
fn runtime_integration_blocks_guardrail_violations_before_persisting_run_records() {
    let cases = [
        (
            "git-push",
            r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "attempted forbidden git push",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": [],
"blocked_actions": ["git push"],
"needs_patch": false
  },
  "diagnostics": []
}"#,
            "runtime blocked: git push blocked by work guardrails",
        ),
        (
            "unrelated-write",
            r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "attempted unrelated doc write",
"changed_paths": ["docs/08-runtime-integration.md"],
"suggested_test_selectors": [],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
            "runtime blocked: unrelated write blocked: docs/08-runtime-integration.md",
        ),
        (
            "destructive-rm",
            r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "attempted destructive remove",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": [],
"blocked_actions": ["rm -rf crates"],
"needs_patch": false
  },
  "diagnostics": []
}"#,
            "runtime blocked: destructive recursive remove blocked outside temp workspace: crates",
        ),
    ];

    for (name, assistant_text, expected_error) in cases {
        let temp = TestDir::new(&format!("runtime-integration-guardrail-{name}"));
        let repo_root = temp.path();
        let triad = LocalTriad::new(test_config(repo_root));

        write_supporting_runtime_files(repo_root);
        triad
            .init_scaffold(false)
            .expect("scaffold should create run dir");
        write_claim_file(repo_root, "REQ-auth-001", "Login");
        let src_dir = repo_root.join("src");
        fs::create_dir_all(&src_dir).expect("src dir should exist");
        fs::write(src_dir.join("auth.rs"), "pub fn login() {}\n")
            .expect("source file should be written");
        let process_runner = FakeProcessRunner::codex_success(assistant_text, Vec::new());

        let error = run_claim_with_backend_adapter(
            &triad,
            RunClaimRequest {
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                dry_run: false,
                model: None,
                effort: None,
            },
            &process_runner,
        )
        .expect_err("guardrail violation should surface");

        assert_eq!(process_runner.calls().len(), 1);
        assert_eq!(error.to_string(), expected_error);
        assert_eq!(
            triad
                .next_run_id()
                .expect("blocked work must not persist a run record")
                .as_str(),
            "RUN-000001"
        );
    }
}

#[test]
fn runtime_integration_staged_copy_back_updates_repo_after_guarded_run() {
    let temp = TestDir::new("runtime-integration-staged-copy-back");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");
    let process_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "staged write replay",
"changed_paths": ["src/auth.rs", "Cargo.lock"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        vec![
            (
                "src/auth.rs".to_string(),
                "pub fn login() -> bool { true }\n".to_string(),
            ),
            ("Cargo.lock".to_string(), "# generated\n".to_string()),
            (
                "target/debug/triad-build.log".to_string(),
                "compiled\n".to_string(),
            ),
        ],
    );

    let report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        },
        &process_runner,
    )
    .expect("staged run should succeed");

    assert_eq!(report.summary, "staged write replay");
    assert_eq!(process_runner.calls().len(), 1);
    assert_eq!(
        fs::read_to_string(repo_root.join("src/auth.rs")).expect("repo file should read"),
        "pub fn login() -> bool { true }\n"
    );
    assert!(!repo_root.join("Cargo.lock").exists());
    assert!(!repo_root.join("target/debug/triad-build.log").exists());
}

#[test]
fn runtime_integration_staged_copy_back_blocks_hidden_diff_before_repo_mutation() {
    let temp = TestDir::new("runtime-integration-staged-hidden-diff");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::create_dir_all(repo_root.join("docs")).expect("docs dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");
    fs::write(repo_root.join("docs/out-of-scope.md"), "original doc\n")
        .expect("docs file should be written");
    let original_docs =
        fs::read_to_string(repo_root.join("docs/out-of-scope.md")).expect("docs file should read");
    let process_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "hidden staged diff replay",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        vec![
            (
                "src/auth.rs".to_string(),
                "pub fn login() -> bool { true }\n".to_string(),
            ),
            (
                "docs/out-of-scope.md".to_string(),
                "mutated doc\n".to_string(),
            ),
        ],
    );

    let error = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        },
        &process_runner,
    )
    .expect_err("hidden staged diff should be blocked");

    assert_eq!(
        error.to_string(),
        "runtime blocked: workspace diff missing from reported changed_paths: docs/out-of-scope.md"
    );
    assert_eq!(
        fs::read_to_string(repo_root.join("src/auth.rs")).expect("repo file should read"),
        "pub fn login() {}\n"
    );
    assert_eq!(
        fs::read_to_string(repo_root.join("docs/out-of-scope.md")).expect("docs file should read"),
        original_docs
    );
}

#[test]
fn runtime_integration_process_failures_do_not_persist_run_records() {
    let temp = TestDir::new("runtime-integration-process-failure");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    let process_runner = FakeProcessRunner::failure("", "process failed", 7);

    let error = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        },
        &process_runner,
    )
    .expect_err("process failure should surface");

    assert_eq!(process_runner.calls().len(), 1);
    assert_eq!(
        error.to_string(),
        "invalid state: codex exec failed with exit code 7: process failed"
    );
    assert_eq!(
        triad
            .next_run_id()
            .expect("run record must not persist after process failure")
            .as_str(),
        "RUN-000001"
    );
    let malformed_output_runner = FakeProcessRunner::codex_success("", Vec::new());
    let error = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        },
        &malformed_output_runner,
    )
    .expect_err("malformed codex output should surface");

    assert_eq!(malformed_output_runner.calls().len(), 1);
    assert!(
        error
            .to_string()
            .starts_with("parse error: failed to parse work response JSON")
    );
    assert_eq!(
        triad
            .next_run_id()
            .expect("run record must not persist after malformed output")
            .as_str(),
        "RUN-000001"
    );
}

#[test]
fn prompt_envelope_scopes_context_to_agents_and_selected_claim_only() {
    let temp = TestDir::new("prompt-envelope-scope");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    write_claim_file(repo_root, "REQ-auth-002", "Logout");

    let envelope = triad
        .work_prompt_envelope(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("prompt envelope should build");

    assert_eq!(envelope.claim.id.as_str(), "REQ-auth-001");
    assert_eq!(envelope.session_config.cwd, repo_root.display().to_string());
    assert_eq!(
        envelope.session_config.attachments,
        vec![
            PromptAttachment::AtPath {
                path: "AGENTS.md".to_string(),
                placeholder: None,
            },
            PromptAttachment::AtPath {
                path: "spec/claims/REQ-auth-001.md".to_string(),
                placeholder: None,
            },
        ]
    );
    assert!(envelope.prompt.contains("Selected claim: REQ-auth-001"));
    assert!(envelope.prompt.contains("Forbidden actions:"));
    assert!(envelope.prompt.contains("Return JSON only"));
    assert!(
        envelope
            .prompt
            .contains("changed_paths must list every modified repo file")
    );
    assert!(!envelope.prompt.contains("REQ-auth-002"));
    let schema = envelope
        .session_config
        .output_schema
        .expect("output schema should be set");
    assert_eq!(schema["title"], "Triad Agent Run Response");
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["command"]["const"], "run");
    assert_eq!(
        schema["properties"]["data"]["required"],
        serde_json::json!([
            "claim_id",
            "summary",
            "changed_paths",
            "suggested_test_selectors",
            "blocked_actions",
            "needs_patch"
        ])
    );
    assert!(schema["properties"]["data"]["properties"]["run_id"].is_null());
    assert!(schema.get("allOf").is_none());
}

#[test]
fn prompt_envelope_rejects_missing_claim() {
    let temp = TestDir::new("prompt-envelope-missing-claim");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");

    let error = triad
        .work_prompt_envelope(&ClaimId::new("REQ-auth-999").expect("claim id should parse"))
        .expect_err("missing claim should fail");

    assert_eq!(
        error.to_string(),
        "invalid state: claim not found: REQ-auth-999"
    );
}

#[test]
fn prompt_envelope_rejects_work_schema_missing_required_field_contract() {
    let temp = TestDir::new("prompt-envelope-invalid-schema");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::write(
        repo_root.join("schemas/agent.run.schema.json"),
        r#"{
  "title": "Triad Agent Run Response",
  "allOf": [
{},
{
  "properties": {
    "command": {"const": "run"},
    "data": {
      "type": "object",
      "additionalProperties": false,
      "required": ["claim_id", "summary", "changed_paths", "suggested_test_selectors", "needs_patch"],
      "properties": {
        "claim_id": {"type": "string"},
        "summary": {"type": "string"},
        "changed_paths": {"type": "array", "items": {"type": "string"}},
        "suggested_test_selectors": {"type": "array", "items": {"type": "string"}},
        "needs_patch": {"type": "boolean"}
      }
    }
  }
}
  ]
}"#,
    )
    .expect("invalid schema should be written");

    let error = triad
        .work_prompt_envelope(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect_err("invalid schema should fail");
    assert_eq!(
        error.to_string(),
        "invalid state: agent.run schema missing required data field: blocked_actions"
    );
}

#[test]
fn guardrails_require_selected_claim_before_run() {
    let temp = TestDir::new("guardrails-missing-claim");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");

    let error = triad
        .work_guardrails(
            &ClaimId::new("REQ-auth-999").expect("claim id should parse"),
            &[utf8(repo_root.join("crates"))],
        )
        .expect_err("missing claim should fail");
    assert_eq!(
        error.to_string(),
        "invalid state: claim not found: REQ-auth-999"
    );
}

#[test]
fn guardrails_block_forbidden_tool_use_as_runtime_blocked() {
    let temp = TestDir::new("guardrails-blocked-tool-use");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");

    let guardrails = triad
        .work_guardrails(
            &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            &[
                utf8(repo_root.join("crates")),
                utf8(repo_root.join("tests")),
                utf8(repo_root.join(".triad/tmp")),
            ],
        )
        .expect("guardrails should build");

    let spec_edit = guardrails
        .check(&WorkToolUse::WritePath {
            path: Utf8PathBuf::from("spec/claims/REQ-auth-001.md"),
        })
        .expect_err("spec edit should be blocked");
    assert_eq!(
        spec_edit.to_string(),
        "runtime blocked: direct spec edit blocked: spec/claims/REQ-auth-001.md"
    );

    let git_commit = guardrails
        .check(&WorkToolUse::Exec {
            program: "git".to_string(),
            args: vec!["commit".to_string(), "-m".to_string(), "x".to_string()],
        })
        .expect_err("git commit should be blocked");
    assert_eq!(
        git_commit.to_string(),
        "runtime blocked: git commit blocked by work guardrails"
    );

    let git_push = guardrails
        .check(&WorkToolUse::Exec {
            program: "git".to_string(),
            args: vec!["push".to_string()],
        })
        .expect_err("git push should be blocked");
    assert_eq!(
        git_push.to_string(),
        "runtime blocked: git push blocked by work guardrails"
    );

    let unrelated_write = guardrails
        .check(&WorkToolUse::WritePath {
            path: Utf8PathBuf::from("docs/08-runtime-integration.md"),
        })
        .expect_err("unrelated write should be blocked");
    assert_eq!(
        unrelated_write.to_string(),
        "runtime blocked: unrelated write blocked: docs/08-runtime-integration.md"
    );

    let destructive_rm = guardrails
        .check(&WorkToolUse::RemovePath {
            path: Utf8PathBuf::from("crates"),
            recursive: true,
        })
        .expect_err("recursive rm outside temp workspace should be blocked");
    assert_eq!(
        destructive_rm.to_string(),
        "runtime blocked: destructive recursive remove blocked outside temp workspace: crates"
    );
}

#[test]
fn guardrails_allow_scoped_code_write_and_temp_workspace_remove() {
    let temp = TestDir::new("guardrails-allowed-tool-use");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    write_claim_file(repo_root, "REQ-auth-001", "Login");

    let guardrails = triad
        .work_guardrails(
            &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            &[
                utf8(repo_root.join("crates")),
                utf8(repo_root.join("tests")),
                utf8(repo_root.join(".triad/tmp")),
            ],
        )
        .expect("guardrails should build");

    guardrails
        .check(&WorkToolUse::WritePath {
            path: Utf8PathBuf::from("crates/triad-runtime/src/lib.rs"),
        })
        .expect("code write under allowed root should pass");

    guardrails
        .check(&WorkToolUse::RemovePath {
            path: Utf8PathBuf::from(".triad/tmp/session-001"),
            recursive: true,
        })
        .expect("recursive rm inside temp workspace should pass");
}

#[test]
fn init_scaffold_creates_repo_scaffold() {
    let temp = TestDir::new("init-scaffold");
    let repo_root = temp.path();

    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("init scaffold should succeed");

    assert!(repo_root.join("triad.toml").is_file());
    assert!(repo_root.join("AGENTS.md").is_file());
    assert!(repo_root.join(".gitignore").is_file());
    assert!(repo_root.join("docs").is_dir());
    assert!(repo_root.join("schemas").is_dir());
    assert!(repo_root.join("schemas/agent.run.schema.json").is_file());
    assert!(repo_root.join("schemas/envelope.schema.json").is_file());
    assert!(repo_root.join("spec/claims").is_dir());
    assert!(repo_root.join(".triad").is_dir());
    assert!(repo_root.join(".triad/patches").is_dir());
    assert!(repo_root.join(".triad/runs").is_dir());
    assert!(repo_root.join(".triad/evidence.ndjson").is_file());
}

#[test]
fn init_scaffold_writes_full_default_schema_set() {
    let temp = TestDir::new("init-schema-files");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("init scaffold should succeed");

    for (file_name, contents) in DEFAULT_SCHEMA_FILES {
        let path = repo_root.join("schemas").join(file_name);
        assert!(
            path.is_file(),
            "missing scaffolded schema file: {file_name}"
        );
        assert_eq!(
            fs::read_to_string(path).expect("schema file should be readable"),
            *contents
        );
    }
}

#[test]
fn init_idempotent_preserves_existing_evidence() {
    let temp = TestDir::new("init-idempotent");
    let repo_root = temp.path();
    let evidence_path = repo_root.join(".triad/evidence.ndjson");
    let gitignore_path = repo_root.join(".gitignore");
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("initial scaffold should succeed");
    fs::write(&evidence_path, "{\"seed\":true}\n").expect("seed evidence should be written");
    fs::write(&gitignore_path, "# keep custom ignore\n").expect("gitignore should be written");

    triad
        .init_scaffold(false)
        .expect("second scaffold should preserve existing files");

    assert_eq!(
        fs::read_to_string(&evidence_path).expect("evidence should remain readable"),
        "{\"seed\":true}\n"
    );
    assert_eq!(
        fs::read_to_string(&gitignore_path).expect("gitignore should remain readable"),
        "# keep custom ignore\n"
    );
}

#[test]
fn init_force_overwrite_recreates_existing_evidence() {
    let temp = TestDir::new("init-force");
    let repo_root = temp.path();
    let evidence_path = repo_root.join(".triad/evidence.ndjson");
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("initial scaffold should succeed");
    fs::write(&evidence_path, "{\"seed\":true}\n").expect("seed evidence should be written");

    triad
        .init_scaffold(true)
        .expect("force scaffold should refresh existing files");

    assert_eq!(
        fs::read_to_string(&evidence_path).expect("evidence should remain readable"),
        ""
    );
}

#[test]
fn runtime_builder_config_uses_canonicalized_paths() {
    let temp = TestDir::new("runtime-builder-config");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    assert_eq!(
        triad.config.paths.claim_dir,
        utf8(repo_root.join("spec/claims"))
    );
    assert_eq!(triad.config.paths.state_dir, utf8(repo_root.join(".triad")));
    assert_eq!(
        triad.config.paths.evidence_file,
        utf8(repo_root.join(".triad/evidence.ndjson"))
    );
    assert_eq!(triad.config.repo_root, utf8(repo_root.to_path_buf()));
}

#[test]
fn claim_discovery_returns_sorted_top_level_markdown_files() {
    let temp = TestDir::new("claim-discovery");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(claim_dir.join("REQ-auth-002.md"), "# REQ-auth-002 Two\n")
        .expect("claim file should be written");
    fs::write(claim_dir.join("REQ-auth-001.md"), "# REQ-auth-001 One\n")
        .expect("claim file should be written");
    fs::write(claim_dir.join("notes.txt"), "ignore\n").expect("text file should be written");

    let triad = LocalTriad::new(test_config(repo_root));
    let claim_files = triad
        .claim_file_paths()
        .expect("claim discovery should succeed");

    assert_eq!(
        claim_files,
        vec![
            utf8(claim_dir.join("REQ-auth-001.md")),
            utf8(claim_dir.join("REQ-auth-002.md"))
        ]
    );
}

#[test]
fn claim_discovery_rejects_nested_directories() {
    let temp = TestDir::new("claim-discovery-nested");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    fs::create_dir_all(claim_dir.join("nested")).expect("nested claim dir should exist");

    let triad = LocalTriad::new(test_config(repo_root));
    let error = triad
        .claim_file_paths()
        .expect_err("nested claim directory should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "invalid state: nested claim directory is not allowed: {}",
            claim_dir.join("nested").display()
        )
    );
}

#[test]
fn claim_filename_match_accepts_matching_file_name_and_h1_id() {
    let temp = TestDir::new("claim-filename-match-valid");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        claim_dir.join("REQ-auth-001.md"),
        valid_claim_body("REQ-auth-001", "Login success"),
    )
    .expect("claim file should be written");

    let triad = LocalTriad::new(test_config(repo_root));
    let report = triad
        .ingest_spec()
        .expect("matching file name should ingest");

    assert_eq!(report.claim_count, 1);
}

#[test]
fn claim_filename_match_rejects_mismatched_file_name_and_h1_id() {
    let temp = TestDir::new("claim-filename-match-invalid");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        claim_dir.join("login.md"),
        valid_claim_body("REQ-auth-001", "Login success"),
    )
    .expect("claim file should be written");

    let triad = LocalTriad::new(test_config(repo_root));
    let error = triad
        .ingest_spec()
        .expect_err("mismatched file name should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "parse error: claim file name does not match H1 id: login != REQ-auth-001 in {}",
            claim_dir.join("login.md").display()
        )
    );
}

#[test]
fn claim_sections_accept_required_sections_and_optional_notes() {
    let temp = TestDir::new("claim-sections-valid");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    let claim_path = claim_dir.join("REQ-auth-001.md");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        &claim_path,
        valid_claim_body("REQ-auth-001", "Login success"),
    )
    .expect("claim file should be written");

    let claim = parse_claim_file(&utf8(claim_path)).expect("valid claim should parse");

    assert_eq!(claim.id.as_str(), "REQ-auth-001");
}

#[test]
fn claim_sections_reject_missing_required_section() {
    let temp = TestDir::new("claim-sections-missing");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    let claim_path = claim_dir.join("REQ-auth-001.md");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        &claim_path,
        "\
# REQ-auth-001 Login success

## Claim
User can log in.

## Examples
- valid credentials -> 200
",
    )
    .expect("claim file should be written");

    let error = parse_claim_file(&utf8(claim_path)).expect_err("missing section should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "parse error: missing required section Invariants in {}",
            claim_dir.join("REQ-auth-001.md").display()
        )
    );
}

#[test]
fn claim_sections_reject_extra_section() {
    let temp = TestDir::new("claim-sections-extra");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    let claim_path = claim_dir.join("REQ-auth-001.md");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        &claim_path,
        "\
# REQ-auth-001 Login success

## Claim
User can log in.

## Examples
- valid credentials -> 200

## Invariants
- no plaintext password logs

## Open Questions
- should deleted user return 404?
",
    )
    .expect("claim file should be written");

    let error = parse_claim_file(&utf8(claim_path)).expect_err("extra section should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "parse error: unexpected section Open Questions at line 12 in {}",
            claim_dir.join("REQ-auth-001.md").display()
        )
    );
}

#[test]
fn claim_sections_reject_wrong_section_order() {
    let temp = TestDir::new("claim-sections-order");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    let claim_path = claim_dir.join("REQ-auth-001.md");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        &claim_path,
        "\
# REQ-auth-001 Login success

## Examples
- valid credentials -> 200

## Claim
User can log in.

## Invariants
- no plaintext password logs
",
    )
    .expect("claim file should be written");

    let error = parse_claim_file(&utf8(claim_path)).expect_err("wrong order should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "parse error: section Examples is out of order, expected Claim at line 3 in {}",
            claim_dir.join("REQ-auth-001.md").display()
        )
    );
}

#[test]
fn claim_roundtrip_preserves_raw_examples_and_invariants() {
    let temp = TestDir::new("claim-roundtrip");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    let claim_path = claim_dir.join("REQ-auth-001.md");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        &claim_path,
        "\
# REQ-auth-001 Login success

## Claim
User can log in with valid credentials.

## Examples
- valid credentials -> 200 + session cookie
- wrong password -> 401

## Invariants
- password plaintext never appears in logs
- error body does not reveal account existence

## Notes
- MFA remains out of scope
",
    )
    .expect("claim file should be written");

    let claim = parse_claim_file(&utf8(claim_path)).expect("claim should parse");

    assert_eq!(
        claim.examples[0],
        "valid credentials -> 200 + session cookie"
    );
    assert_eq!(claim.examples[1], "wrong password -> 401");
    assert_eq!(
        claim.invariants[0],
        "password plaintext never appears in logs"
    );
    assert_eq!(
        claim.invariants[1],
        "error body does not reveal account existence"
    );
    assert_eq!(claim.notes.as_deref(), Some("- MFA remains out of scope"));
    assert_eq!(claim.revision, claim_revision_number(&claim));
}

#[test]
fn claim_roundtrip_keeps_multiline_claim_statement_text() {
    let temp = TestDir::new("claim-roundtrip-statement");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    let claim_path = claim_dir.join("REQ-auth-001.md");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        &claim_path,
        "\
# REQ-auth-001 Login success

## Claim
User can log in with valid credentials.
Second line stays attached to the same section.

## Examples
- valid credentials -> 200

## Invariants
- session cookie is issued after success
",
    )
    .expect("claim file should be written");

    let claim = parse_claim_file(&utf8(claim_path)).expect("claim should parse");

    assert_eq!(
        claim.statement,
        "User can log in with valid credentials.\nSecond line stays attached to the same section."
    );
    assert_eq!(claim.examples, vec!["valid credentials -> 200"]);
    assert_eq!(
        claim.invariants,
        vec!["session cookie is issued after success"]
    );
    assert_eq!(claim.notes, None);
    assert_eq!(claim.revision, claim_revision_number(&claim));
}

#[test]
fn claim_revision_bytes_normalize_line_endings_and_trailing_whitespace() {
    let temp = TestDir::new("claim-revision-bytes");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");

    let lf_path = claim_dir.join("REQ-auth-001.md");
    fs::write(
        &lf_path,
        "\
# REQ-auth-001 Login success

## Claim
User can log in with valid credentials.
Second line stays attached.

## Examples
- valid credentials -> 200

## Invariants
- session cookie is issued

## Notes
MFA remains out of scope
",
    )
    .expect("claim file should be written");

    let crlf_path = claim_dir.join("REQ-auth-002.md");
    fs::write(
        &crlf_path,
        "# REQ-auth-002   Login success   \r\n\r\n## Claim\r\nUser can log in with valid credentials.   \r\nSecond line stays attached.\t\r\n\r\n## Examples\r\n- valid credentials -> 200   \r\n\r\n## Invariants\r\n- session cookie is issued\t\r\n\r\n## Notes\r\nMFA remains out of scope   \r\n",
    )
    .expect("claim file should be written");

    let left_claim = parse_claim_file(&utf8(lf_path)).expect("lf claim should parse");
    let right_claim = parse_claim_file(&utf8(crlf_path)).expect("crlf claim should parse");
    let left_bytes = canonical_claim_revision_bytes(&left_claim);
    let right_bytes = canonical_claim_revision_bytes(&right_claim);

    let expected = "\
# REQ-auth-001 Login success

## Claim
User can log in with valid credentials.
Second line stays attached.

## Examples
- valid credentials -> 200

## Invariants
- session cookie is issued

## Notes
MFA remains out of scope
";

    assert_eq!(
        String::from_utf8(left_bytes).expect("bytes should be utf-8"),
        expected
    );
    assert_eq!(
        String::from_utf8(right_bytes).expect("bytes should be utf-8"),
        expected.replace("REQ-auth-001", "REQ-auth-002")
    );
}

#[test]
fn claim_revision_bytes_emit_stable_section_layout_without_notes() {
    let temp = TestDir::new("claim-revision-layout");
    let repo_root = temp.path();
    let claim_dir = repo_root.join("spec/claims");
    let claim_path = claim_dir.join("REQ-auth-001.md");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        &claim_path,
        "\
# REQ-auth-001 Login success

## Claim
User can log in with valid credentials.

## Examples
- valid credentials -> 200

## Invariants
- session cookie is issued
",
    )
    .expect("claim file should be written");

    let claim = parse_claim_file(&utf8(claim_path)).expect("claim should parse");
    let bytes = canonical_claim_revision_bytes(&claim);

    assert_eq!(
        String::from_utf8(bytes).expect("bytes should be utf-8"),
        "\
# REQ-auth-001 Login success

## Claim
User can log in with valid credentials.

## Examples
- valid credentials -> 200

## Invariants
- session cookie is issued
"
    );
}

#[test]
fn parser_golden_parses_repo_claim_fixtures() {
    let claim = parse_claim_file(&repo_claim_path("REQ-auth-001.md"))
        .expect("REQ-auth-001 fixture should parse");
    assert_eq!(claim.id.as_str(), "REQ-auth-001");
    assert_eq!(claim.title, "Login success");
    assert_eq!(
        claim.statement,
        "사용자는 유효한 이메일/비밀번호 조합으로 로그인할 수 있어야 한다."
    );
    assert_eq!(
        claim.examples,
        vec![
            "valid credentials -> 200 + session cookie",
            "wrong password -> 401",
            "deleted user -> 404",
        ]
    );
    assert_eq!(
        claim.invariants,
        vec![
            "비밀번호 원문은 로그에 남지 않는다.",
            "실패 응답은 계정 존재 여부를 과도하게 노출하지 않는다.",
        ]
    );
    assert_eq!(claim.notes.as_deref(), Some("- MFA는 범위 밖"));
    assert_eq!(claim.revision, claim_revision_number(&claim));

    let claim = parse_claim_file(&repo_claim_path("REQ-auth-002.md"))
        .expect("REQ-auth-002 fixture should parse");
    assert_eq!(claim.id.as_str(), "REQ-auth-002");
    assert_eq!(claim.title, "Session invalidation on logout");
    assert_eq!(
        claim.statement,
        "로그아웃 이후 기존 세션 쿠키는 더 이상 인증된 요청에 사용될 수 없어야 한다."
    );
    assert_eq!(
        claim.examples,
        vec!["logout -> 204", "old cookie after logout -> 401"]
    );
    assert_eq!(
        claim.invariants,
        vec![
            "로그아웃은 idempotent 하게 처리된다.",
            "로그아웃 성공 후 동일 세션으로 보호 자원에 접근할 수 없다.",
        ]
    );
    assert_eq!(claim.notes, None);
    assert_eq!(claim.revision, claim_revision_number(&claim));
}

#[test]
fn parser_golden_reports_exact_error_for_invalid_fixture() {
    let temp = TestDir::new("parser-golden-invalid");
    let invalid_path = temp.path().join("login.md");
    fs::write(
        &invalid_path,
        fs::read_to_string(repo_claim_path("REQ-auth-001.md").as_std_path())
            .expect("fixture should be readable"),
    )
    .expect("invalid fixture copy should be written");

    let error =
        parse_claim_file(&utf8(invalid_path.clone())).expect_err("invalid fixture should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "parse error: claim file name does not match H1 id: login != REQ-auth-001 in {}",
            invalid_path.display()
        )
    );
}

#[test]
fn evidence_id_generation_starts_at_first_id_for_empty_log() {
    let temp = TestDir::new("evidence-id-empty");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");

    assert_eq!(
        triad
            .next_evidence_id()
            .expect("empty log should yield first evidence id")
            .as_str(),
        "EVID-000001"
    );
}

#[test]
fn evidence_id_generation_uses_next_monotonic_suffix() {
    let temp = TestDir::new("evidence-id-next");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");
    fs::write(
        triad.config.paths.evidence_file.as_std_path(),
        "\
{\"id\":\"EVID-000001\",\"claim_id\":\"REQ-auth-001\",\"kind\":\"unit\",\"verdict\":\"pass\",\"test_selector\":null,\"command\":\"cargo test auth::one\",\"covered_paths\":[],\"covered_digests\":{},\"spec_revision\":1,\"created_at\":\"2026-03-10T00:00:00+09:00\"}
{\"id\":\"EVID-000003\",\"claim_id\":\"REQ-auth-001\",\"kind\":\"contract\",\"verdict\":\"pass\",\"test_selector\":null,\"command\":\"cargo test auth::two\",\"covered_paths\":[],\"covered_digests\":{},\"spec_revision\":1,\"created_at\":\"2026-03-10T00:01:00+09:00\"}
{\"id\":\"EVID-000002\",\"claim_id\":\"REQ-auth-002\",\"kind\":\"integration\",\"verdict\":\"fail\",\"test_selector\":null,\"command\":\"cargo test auth::three\",\"covered_paths\":[],\"covered_digests\":{},\"spec_revision\":1,\"created_at\":\"2026-03-10T00:02:00+09:00\"}
",
    )
    .expect("evidence log should be written");

    assert_eq!(
        triad
            .next_evidence_id()
            .expect("next evidence id should use max suffix")
            .as_str(),
        "EVID-000004"
    );
}

#[test]
fn evidence_id_generation_rejects_corrupt_rows() {
    let temp = TestDir::new("evidence-id-invalid");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");
    fs::write(
        triad.config.paths.evidence_file.as_std_path(),
        "{\"id\":\"BAD\",\"claim_id\":\"REQ-auth-001\"}\n",
    )
    .expect("invalid evidence row should be written");

    let error = triad
        .next_evidence_id()
        .expect_err("invalid row should block id generation");

    assert_eq!(
        error.to_string(),
        format!(
            "serialization error: invalid evidence row at line 1 in {}: parse error: invalid evidence id: BAD",
            triad.config.paths.evidence_file
        )
    );
}

#[test]
fn evidence_id_generation_rejects_sequence_overflow() {
    let temp = TestDir::new("evidence-id-overflow");
    let repo_root = temp.path();
    let evidence_path = repo_root.join(".triad/evidence.ndjson");
    fs::create_dir_all(repo_root.join(".triad")).expect("state dir should exist");
    fs::write(
        &evidence_path,
        "{\"id\":\"EVID-999999\",\"claim_id\":\"REQ-auth-001\",\"kind\":\"unit\",\"verdict\":\"pass\",\"test_selector\":null,\"command\":\"cargo test auth::one\",\"covered_paths\":[],\"covered_digests\":{},\"spec_revision\":1,\"created_at\":\"2026-03-10T00:00:00+09:00\"}\n",
    )
    .expect("evidence log should be written");

    let error = next_evidence_id(&evidence_path).expect_err("overflow should fail");

    assert_eq!(
        error.to_string(),
        "parse error: invalid evidence id sequence: 1000000"
    );
}

#[test]
fn evidence_append_writes_one_json_object_per_line() {
    let temp = TestDir::new("evidence-append");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");
    triad
        .append_evidence(&test_evidence("EVID-000001", "REQ-auth-001", "auth::one"))
        .expect("first evidence should append");
    triad
        .append_evidence(&test_evidence("EVID-000002", "REQ-auth-002", "auth::two"))
        .expect("second evidence should append");

    let content = fs::read_to_string(triad.config.paths.evidence_file.as_std_path())
        .expect("evidence log should be readable");
    let lines: Vec<&str> = content.lines().collect();

    assert_eq!(lines.len(), 2);
    for line in lines {
        assert!(
            serde_json::from_str::<serde_json::Value>(line)
                .expect("line should parse")
                .is_object()
        );
    }
    assert!(content.ends_with('\n'));
}

#[test]
fn evidence_append_rejects_non_monotonic_id() {
    let temp = TestDir::new("evidence-append-monotonic");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");
    triad
        .append_evidence(&test_evidence("EVID-000001", "REQ-auth-001", "auth::one"))
        .expect("first evidence should append");

    let error = triad
        .append_evidence(&test_evidence("EVID-000001", "REQ-auth-001", "auth::dup"))
        .expect_err("duplicate id should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "invalid state: evidence id must be next monotonic id for {}: expected EVID-000002, got EVID-000001",
            triad.config.paths.evidence_file
        )
    );
}

#[test]
fn evidence_read_roundtrips_appended_rows() {
    let temp = TestDir::new("evidence-read");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");
    let first = test_evidence("EVID-000001", "REQ-auth-001", "auth::one");
    let second = test_evidence("EVID-000002", "REQ-auth-002", "auth::two");

    triad
        .append_evidence(&first)
        .expect("first evidence should append");
    triad
        .append_evidence(&second)
        .expect("second evidence should append");

    let rows = triad.read_evidence().expect("evidence log should read");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id.as_str(), first.id.as_str());
    assert_eq!(rows[0].claim_id, first.claim_id);
    assert_eq!(rows[0].command, first.command);
    assert_eq!(rows[1].id.as_str(), second.id.as_str());
    assert_eq!(rows[1].claim_id, second.claim_id);
    assert_eq!(rows[1].command, second.command);
}

#[test]
fn evidence_read_rejects_invalid_json_rows() {
    let temp = TestDir::new("evidence-read-invalid");
    let repo_root = temp.path();
    let evidence_path = repo_root.join(".triad/evidence.ndjson");
    fs::create_dir_all(repo_root.join(".triad")).expect("state dir should exist");
    fs::write(&evidence_path, "{\"id\":\"EVID-000001\"\n").expect("invalid row should exist");

    let error = read_evidence(&evidence_path).expect_err("invalid json should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "serialization error: invalid evidence row at line 1 in {}: EOF while parsing an object",
            evidence_path.display()
        )
    );
}

#[test]
fn evidence_append_rejects_log_without_terminal_newline() {
    let temp = TestDir::new("evidence-append-newline");
    let repo_root = temp.path();
    let evidence_path = repo_root.join(".triad/evidence.ndjson");
    fs::create_dir_all(repo_root.join(".triad")).expect("state dir should exist");
    fs::write(
        &evidence_path,
        "{\"id\":\"EVID-000001\",\"claim_id\":\"REQ-auth-001\",\"kind\":\"unit\",\"verdict\":\"pass\",\"test_selector\":null,\"command\":\"cargo test auth::one\",\"covered_paths\":[],\"covered_digests\":{},\"spec_revision\":1,\"created_at\":\"2026-03-10T00:00:00+09:00\"}",
    )
    .expect("evidence row should be written");

    let error = append_evidence(
        &evidence_path,
        &test_evidence("EVID-000002", "REQ-auth-001", "auth::two"),
    )
    .expect_err("missing terminal newline should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "invalid state: evidence log must end with newline before append: {}",
            evidence_path.display()
        )
    );
}

#[test]
fn covered_digests_change_when_file_bytes_change() {
    let temp = TestDir::new("covered-digests-change");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let auth_path = repo_root.join("src/auth.rs");
    fs::create_dir_all(auth_path.parent().expect("parent should exist"))
        .expect("src dir should exist");
    fs::write(&auth_path, "fn login() -> bool { true }\n").expect("file should be written");

    let first = triad
        .covered_digests(&[Utf8PathBuf::from("src/auth.rs")])
        .expect("digest should compute");
    fs::write(&auth_path, "fn login() -> bool { false }\n").expect("file should be rewritten");
    let second = triad
        .covered_digests(&[Utf8PathBuf::from("src/auth.rs")])
        .expect("digest should recompute");

    let key = camino::Utf8Path::new("src/auth.rs");
    assert_ne!(first[key], second[key]);
    assert!(first[key].starts_with("sha256:"));
    assert!(second[key].starts_with("sha256:"));
}

#[test]
fn covered_digests_are_keyed_by_relative_path() {
    let temp = TestDir::new("covered-digests-keys");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let src_dir = repo_root.join("src");
    fs::create_dir_all(&src_dir).expect("src dir should exist");
    fs::write(src_dir.join("auth.rs"), "fn auth() {}\n").expect("auth file should be written");
    fs::write(src_dir.join("session.rs"), "fn session() {}\n")
        .expect("session file should be written");

    let digests = triad
        .covered_digests(&[
            Utf8PathBuf::from("src/session.rs"),
            Utf8PathBuf::from("src/auth.rs"),
        ])
        .expect("digest map should compute");

    assert_eq!(
        digests.keys().cloned().collect::<Vec<_>>(),
        vec![
            Utf8PathBuf::from("src/auth.rs"),
            Utf8PathBuf::from("src/session.rs")
        ]
    );
}

#[test]
fn covered_digests_reject_parent_escape_paths() {
    let temp = TestDir::new("covered-digests-escape");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    let error = triad
        .covered_digests(&[Utf8PathBuf::from("../secret.txt")])
        .expect_err("parent escape should fail");

    assert_eq!(
        error.to_string(),
        "invalid state: covered path must not escape repo root: ../secret.txt"
    );
}

#[test]
fn freshness_is_true_when_current_digests_match_evidence() {
    let temp = TestDir::new("freshness-match");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let auth_path = repo_root.join("src/auth.rs");
    fs::create_dir_all(auth_path.parent().expect("parent should exist"))
        .expect("src dir should exist");
    fs::write(&auth_path, "fn login() -> bool { true }\n").expect("file should be written");

    let digests = triad
        .covered_digests(&[Utf8PathBuf::from("src/auth.rs")])
        .expect("digest should compute");
    let evidence = Evidence {
        covered_digests: digests,
        ..test_evidence("EVID-000001", "REQ-auth-001", "auth::login_success")
    };

    assert!(
        triad
            .evidence_is_fresh(&evidence)
            .expect("freshness should evaluate"),
        "matching digests should be fresh"
    );
}

#[test]
fn freshness_is_false_when_any_covered_path_digest_changes() {
    let temp = TestDir::new("freshness-changed");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let src_dir = repo_root.join("src");
    fs::create_dir_all(&src_dir).expect("src dir should exist");
    fs::write(src_dir.join("auth.rs"), "fn auth() -> bool { true }\n")
        .expect("auth file should be written");
    fs::write(
        src_dir.join("session.rs"),
        "fn session() -> bool { true }\n",
    )
    .expect("session file should be written");

    let covered_paths = vec![
        Utf8PathBuf::from("src/auth.rs"),
        Utf8PathBuf::from("src/session.rs"),
    ];
    let digests = triad
        .covered_digests(&covered_paths)
        .expect("digests should compute");
    fs::write(
        src_dir.join("session.rs"),
        "fn session() -> bool { false }\n",
    )
    .expect("session file should be rewritten");
    let evidence = Evidence {
        covered_paths,
        covered_digests: digests,
        ..test_evidence("EVID-000001", "REQ-auth-001", "auth::login_success")
    };

    assert!(
        !triad
            .evidence_is_fresh(&evidence)
            .expect("freshness should evaluate"),
        "one changed path should make evidence stale"
    );
}

#[test]
fn freshness_is_false_when_covered_path_is_missing() {
    let temp = TestDir::new("freshness-missing");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let auth_path = repo_root.join("src/auth.rs");
    fs::create_dir_all(auth_path.parent().expect("parent should exist"))
        .expect("src dir should exist");
    fs::write(&auth_path, "fn login() -> bool { true }\n").expect("file should be written");

    let digests = triad
        .covered_digests(&[Utf8PathBuf::from("src/auth.rs")])
        .expect("digest should compute");
    fs::remove_file(&auth_path).expect("file should be removed");
    let evidence = Evidence {
        covered_digests: digests,
        ..test_evidence("EVID-000001", "REQ-auth-001", "auth::login_success")
    };

    assert!(
        !triad
            .evidence_is_fresh(&evidence)
            .expect("freshness should evaluate"),
        "missing path should make evidence stale"
    );
}

#[test]
fn relevant_evidence_selects_latest_fresh_row_per_verdict() {
    let temp = TestDir::new("relevant-evidence-latest");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let src_dir = repo_root.join("src");
    fs::create_dir_all(&src_dir).expect("src dir should exist");
    fs::write(
        src_dir.join("pass_old.rs"),
        "fn pass_old() -> bool { true }\n",
    )
    .expect("pass old file should be written");
    fs::write(
        src_dir.join("pass_new.rs"),
        "fn pass_new() -> bool { true }\n",
    )
    .expect("pass new file should be written");
    fs::write(
        src_dir.join("fail.rs"),
        "fn fail_case() -> bool { false }\n",
    )
    .expect("fail file should be written");
    fs::write(src_dir.join("unknown.rs"), "fn probe() {}\n")
        .expect("unknown file should be written");

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::pass_old",
            &["src/pass_old.rs"],
        ))
        .expect("older pass evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-001",
            Verdict::Fail,
            "auth::fail_case",
            &["src/fail.rs"],
        ))
        .expect("fail evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000003",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::pass_new",
            &["src/pass_new.rs"],
        ))
        .expect("newer pass evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000004",
            "REQ-auth-001",
            Verdict::Unknown,
            "auth::probe",
            &["src/unknown.rs"],
        ))
        .expect("unknown evidence should append");

    let relevant = triad
        .relevant_evidence_for_claim(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("relevant evidence should resolve");

    assert_eq!(
        relevant
            .pass
            .expect("pass evidence should exist")
            .id
            .as_str(),
        "EVID-000003"
    );
    assert_eq!(
        relevant
            .fail
            .expect("fail evidence should exist")
            .id
            .as_str(),
        "EVID-000002"
    );
    assert_eq!(
        relevant
            .unknown
            .expect("unknown evidence should exist")
            .id
            .as_str(),
        "EVID-000004"
    );
}

#[test]
fn relevant_evidence_ignores_stale_rows_even_when_they_are_newer() {
    let temp = TestDir::new("relevant-evidence-stale");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let auth_path = repo_root.join("src/auth.rs");
    let unknown_path = repo_root.join("src/unknown.rs");
    fs::create_dir_all(auth_path.parent().expect("parent should exist"))
        .expect("src dir should exist");
    fs::write(&auth_path, "fn auth() -> bool { true }\n").expect("auth file should be written");
    fs::write(&unknown_path, "fn probe() {}\n").expect("unknown file should be written");

    triad
        .init_scaffold(false)
        .expect("scaffold should create evidence log");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::pass_old",
            &["src/auth.rs"],
        ))
        .expect("fresh pass evidence should append");
    fs::write(&auth_path, "fn auth() -> bool { false }\n").expect("auth file should change");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::pass_new",
            &["src/auth.rs"],
        ))
        .expect("newer pass evidence should append");
    fs::write(&auth_path, "fn auth() -> bool { true }\n").expect("auth file should revert");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000003",
            "REQ-auth-001",
            Verdict::Unknown,
            "auth::probe",
            &["src/unknown.rs"],
        ))
        .expect("fresh unknown evidence should append");
    fs::write(&unknown_path, "fn probe() { unreachable!() }\n")
        .expect("unknown file should change");

    let relevant = triad
        .relevant_evidence_for_claim(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("relevant evidence should resolve");

    assert_eq!(
        relevant
            .pass
            .expect("fresh pass evidence should exist")
            .id
            .as_str(),
        "EVID-000001"
    );
    assert!(relevant.fail.is_none(), "no fail evidence should exist");
    assert!(
        relevant.unknown.is_none(),
        "stale unknown evidence should be filtered out"
    );
}

#[test]
fn drift_mapping_reports_healthy_for_latest_fresh_pass_without_pending_patch() {
    let temp = TestDir::new("drift-healthy");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    write_claim_file(repo_root, "REQ-auth-001", "Healthy claim");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/auth.rs"),
        "fn auth() -> bool { true }\n",
    )
    .expect("auth file should be written");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::healthy",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let drift = triad
        .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("drift should resolve");

    assert_eq!(drift.status, DriftStatus::Healthy);
    assert_eq!(
        drift.reasons,
        vec!["fresh pass evidence exists and no pending patch is present"]
    );
    assert_eq!(drift.fresh_evidence_ids.len(), 1);
    assert_eq!(drift.fresh_evidence_ids[0].as_str(), "EVID-000001");
    assert!(drift.pending_patch_id.is_none());
}

#[test]
fn drift_mapping_reports_needs_spec_when_fresh_pass_has_pending_patch() {
    let temp = TestDir::new("drift-needs-spec");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    write_claim_file(repo_root, "REQ-auth-001", "Needs spec claim");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/auth.rs"),
        "fn auth() -> bool { true }\n",
    )
    .expect("auth file should be written");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::needs_spec",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");
    triad
        .store_patch_draft(&test_patch_draft("PATCH-000001"))
        .expect("pending patch should store");

    let drift = triad
        .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("drift should resolve");

    assert_eq!(drift.status, DriftStatus::NeedsSpec);
    assert_eq!(
        drift.reasons,
        vec!["fresh pass evidence exists and a pending patch is present"]
    );
    assert_eq!(
        drift
            .pending_patch_id
            .expect("pending patch should exist")
            .as_str(),
        "PATCH-000001"
    );
}

#[test]
fn drift_mapping_reports_contradicted_when_latest_fresh_evidence_fails() {
    let temp = TestDir::new("drift-contradicted");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    write_claim_file(repo_root, "REQ-auth-001", "Contradicted claim");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/auth.rs"),
        "fn auth() -> bool { true }\n",
    )
    .expect("auth file should be written");
    fs::write(
        repo_root.join("src/fail.rs"),
        "fn fail_case() -> bool { false }\n",
    )
    .expect("fail file should be written");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::pass",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-001",
            Verdict::Fail,
            "auth::fail",
            &["src/fail.rs"],
        ))
        .expect("fail evidence should append");

    let drift = triad
        .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("drift should resolve");

    assert_eq!(drift.status, DriftStatus::Contradicted);
    assert_eq!(drift.reasons, vec!["latest fresh evidence is failing"]);
    assert_eq!(drift.fresh_evidence_ids.len(), 2);
}

#[test]
fn drift_mapping_reports_blocked_when_latest_fresh_evidence_is_unknown() {
    let temp = TestDir::new("drift-blocked");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    write_claim_file(repo_root, "REQ-auth-001", "Blocked claim");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/auth.rs"),
        "fn auth() -> bool { true }\n",
    )
    .expect("auth file should be written");
    fs::write(repo_root.join("src/probe.rs"), "fn probe() {}\n")
        .expect("probe file should be written");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::pass",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-001",
            Verdict::Unknown,
            "auth::probe",
            &["src/probe.rs"],
        ))
        .expect("unknown evidence should append");

    let drift = triad
        .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("drift should resolve");

    assert_eq!(drift.status, DriftStatus::Blocked);
    assert_eq!(drift.reasons, vec!["latest fresh evidence is unknown"]);
    assert_eq!(drift.fresh_evidence_ids.len(), 2);
}

#[test]
fn drift_mapping_reports_needs_test_when_only_stale_observed_paths_exist() {
    let temp = TestDir::new("drift-needs-test");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    write_claim_file(repo_root, "REQ-auth-001", "Needs test claim");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    let auth_path = repo_root.join("src/auth.rs");
    fs::write(&auth_path, "fn auth() -> bool { true }\n").expect("auth file should be written");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::stale",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");
    fs::write(&auth_path, "fn auth() -> bool { false }\n").expect("auth file should change");

    let drift = triad
        .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("drift should resolve");

    assert_eq!(drift.status, DriftStatus::NeedsTest);
    assert_eq!(
        drift.reasons,
        vec!["no fresh evidence exists and implementation paths were previously observed"]
    );
    assert!(drift.fresh_evidence_ids.is_empty());
}

#[test]
fn drift_mapping_reports_needs_code_when_no_fresh_evidence_or_observed_paths_exist() {
    let temp = TestDir::new("drift-needs-code");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    write_claim_file(repo_root, "REQ-auth-001", "Needs code claim");

    triad.init_scaffold(false).expect("scaffold should succeed");

    let drift = triad
        .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("drift should resolve");

    assert_eq!(drift.status, DriftStatus::NeedsCode);
    assert_eq!(
        drift.reasons,
        vec!["no fresh evidence exists and no implementation paths were observed"]
    );
    assert!(drift.fresh_evidence_ids.is_empty());
}

#[test]
fn mismatch_detection_accepts_fresh_pass_with_explicit_patch_signal() {
    let temp = TestDir::new("mismatch-detection-pass");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    let report = test_run_report("RUN-000001");
    triad
        .store_run_record(&report, "sha256:prompt", &BTreeMap::new())
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let mismatch = deterministic_mismatch_for_claim(
        &triad,
        &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
    )
    .expect("mismatch detection should succeed")
    .expect("deterministic mismatch should be found");

    assert_eq!(mismatch.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(
        mismatch.claim_path,
        utf8(repo_root.join("spec/claims/REQ-auth-001.md"))
    );
    assert_eq!(mismatch.run_id.as_str(), "RUN-000001");
    assert_eq!(
        mismatch.based_on_evidence,
        vec![EvidenceId::new("EVID-000001").expect("evidence id should parse")]
    );
    assert_eq!(
        mismatch.changed_paths,
        vec![Utf8PathBuf::from("src/auth.rs")]
    );
    assert_eq!(
        mismatch.reason,
        "latest run RUN-000001 marked needs_patch after fresh pass evidence EVID-000001 on src/auth.rs: Updated auth handler and tests."
    );
}

#[test]
fn mismatch_detection_rejects_missing_patch_signal() {
    let temp = TestDir::new("mismatch-detection-no-patch-signal");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    let mut report = test_run_report("RUN-000001");
    report.needs_patch = false;
    triad
        .store_run_record(&report, "sha256:prompt", &BTreeMap::new())
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let mismatch = deterministic_mismatch_for_claim(
        &triad,
        &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
    )
    .expect("mismatch detection should succeed");

    assert!(mismatch.is_none());
}

#[test]
fn mismatch_detection_rejects_non_passing_or_non_overlapping_evidence() {
    let temp = TestDir::new("mismatch-detection-reject-fail");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Fail,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("failing evidence should append");

    let mismatch = deterministic_mismatch_for_claim(
        &triad,
        &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
    )
    .expect("mismatch detection should succeed");
    assert!(mismatch.is_none());

    let temp = TestDir::new("mismatch-detection-reject-overlap");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");
    fs::write(repo_root.join("src/logout.rs"), "pub fn logout() {}\n")
        .expect("second source file should be written");

    let mut report = test_run_report("RUN-000001");
    report.changed_paths = vec!["src/logout.rs".to_string()];
    triad
        .store_run_record(&report, "sha256:prompt", &BTreeMap::new())
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let mismatch = deterministic_mismatch_for_claim(
        &triad,
        &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
    )
    .expect("mismatch detection should succeed");
    assert!(mismatch.is_none());
}

#[test]
fn minimal_diff_limits_hunk_to_changed_claim_line() {
    let temp = TestDir::new("minimal-diff-claim-line");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    let current = Claim {
        id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        title: "Login success".to_string(),
        statement: "User can log in with valid credentials.".to_string(),
        examples: vec!["valid credentials -> 200 + session cookie".to_string()],
        invariants: vec!["password plaintext never appears in logs".to_string()],
        notes: Some("MFA is out of scope".to_string()),
        revision: 0,
    };
    let mut proposed = current.clone();
    proposed.statement = "User can log in with a verified email/password pair.".to_string();

    let diff = triad
        .minimal_claim_diff(&current, &proposed)
        .expect("minimal diff should build");

    assert_eq!(
        diff,
        "\
--- a/spec/claims/REQ-auth-001.md
+++ b/spec/claims/REQ-auth-001.md
@@ -4 +4 @@
-User can log in with valid credentials.
+User can log in with a verified email/password pair.
"
    );
}

#[test]
fn minimal_diff_limits_hunk_to_changed_example_line() {
    let temp = TestDir::new("minimal-diff-example-line");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    let current = Claim {
        id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        title: "Login success".to_string(),
        statement: "User can log in with valid credentials.".to_string(),
        examples: vec!["valid credentials -> 200 + session cookie".to_string()],
        invariants: vec!["password plaintext never appears in logs".to_string()],
        notes: Some("MFA is out of scope".to_string()),
        revision: 0,
    };
    let mut proposed = current.clone();
    proposed.examples = vec!["valid credentials -> 204 + session cookie".to_string()];

    let diff = triad
        .minimal_claim_diff(&current, &proposed)
        .expect("minimal diff should build");

    assert_eq!(
        diff,
        "\
--- a/spec/claims/REQ-auth-001.md
+++ b/spec/claims/REQ-auth-001.md
@@ -7 +7 @@
-- valid credentials -> 200 + session cookie
+- valid credentials -> 204 + session cookie
"
    );
}

#[test]
fn minimal_diff_includes_only_inserted_notes_section_when_notes_are_added() {
    let temp = TestDir::new("minimal-diff-notes-insert");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    let current = Claim {
        id: ClaimId::new("REQ-auth-002").expect("claim id should parse"),
        title: "Session invalidation on logout".to_string(),
        statement: "Logged-out sessions cannot authenticate future requests.".to_string(),
        examples: vec![
            "logout -> 204".to_string(),
            "old cookie after logout -> 401".to_string(),
        ],
        invariants: vec!["logout is idempotent".to_string()],
        notes: None,
        revision: 0,
    };
    let mut proposed = current.clone();
    proposed.notes = Some("token revocation remains in scope".to_string());

    let diff = triad
        .minimal_claim_diff(&current, &proposed)
        .expect("minimal diff should build");

    assert_eq!(
        diff,
        "\
--- a/spec/claims/REQ-auth-002.md
+++ b/spec/claims/REQ-auth-002.md
@@ -11,0 +12,3 @@
+
+## Notes
+token revocation remains in scope
"
    );
}

#[test]
fn next_selection_prefers_highest_priority_then_lexical_claim_id() {
    let temp = TestDir::new("next-selection-priority");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/a.rs"), "fn a() -> bool { true }\n")
        .expect("a file should be written");
    fs::write(repo_root.join("src/b.rs"), "fn b() -> bool { false }\n")
        .expect("b file should be written");
    fs::write(repo_root.join("src/c.rs"), "fn c() -> bool { true }\n")
        .expect("c file should be written");
    fs::write(repo_root.join("src/z.rs"), "fn z() -> bool { false }\n")
        .expect("z file should be written");
    write_claim_file(repo_root, "REQ-auth-001", "First contradicted");
    write_claim_file(repo_root, "REQ-auth-002", "Needs test");
    write_claim_file(repo_root, "REQ-auth-003", "Needs spec");
    write_claim_file(repo_root, "REQ-auth-010", "Second contradicted");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Fail,
            "auth::fail_one",
            &["src/a.rs"],
        ))
        .expect("first fail evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-002",
            Verdict::Pass,
            "auth::stale",
            &["src/b.rs"],
        ))
        .expect("needs-test evidence should append");
    fs::write(repo_root.join("src/b.rs"), "fn b() -> bool { true }\n")
        .expect("b file should change");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000003",
            "REQ-auth-003",
            Verdict::Pass,
            "auth::needs_spec",
            &["src/c.rs"],
        ))
        .expect("needs-spec evidence should append");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000001",
            "REQ-auth-003",
            PatchState::Pending,
        ))
        .expect("pending patch should store");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000004",
            "REQ-auth-010",
            Verdict::Fail,
            "auth::fail_two",
            &["src/z.rs"],
        ))
        .expect("second fail evidence should append");

    let next = triad.next_claim().expect("next claim should resolve");

    assert_eq!(next.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(next.status, DriftStatus::Contradicted);
    assert_eq!(next.next_action, NextAction::Work);
    assert_eq!(next.reason, "latest fresh evidence is failing");
}

#[test]
fn next_selection_falls_back_to_first_healthy_claim_when_all_claims_are_healthy() {
    let temp = TestDir::new("next-selection-healthy");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/a.rs"), "fn a() -> bool { true }\n")
        .expect("a file should be written");
    fs::write(repo_root.join("src/b.rs"), "fn b() -> bool { true }\n")
        .expect("b file should be written");
    write_claim_file(repo_root, "REQ-auth-010", "Later healthy");
    write_claim_file(repo_root, "REQ-auth-002", "Earlier healthy");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-010",
            Verdict::Pass,
            "auth::later",
            &["src/a.rs"],
        ))
        .expect("later healthy evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-002",
            Verdict::Pass,
            "auth::earlier",
            &["src/b.rs"],
        ))
        .expect("earlier healthy evidence should append");

    let next = triad.next_claim().expect("next claim should resolve");

    assert_eq!(next.claim_id.as_str(), "REQ-auth-002");
    assert_eq!(next.status, DriftStatus::Healthy);
    assert_eq!(next.next_action, NextAction::Status);
    assert_eq!(
        next.reason,
        "fresh pass evidence exists and no pending patch is present"
    );
}

#[test]
fn status_rollup_counts_match_claim_summaries() {
    let temp = TestDir::new("status-rollup");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/healthy.rs"),
        "fn healthy() -> bool { true }\n",
    )
    .expect("healthy file should be written");
    fs::write(
        repo_root.join("src/stale.rs"),
        "fn stale() -> bool { true }\n",
    )
    .expect("stale file should be written");
    fs::write(
        repo_root.join("src/spec.rs"),
        "fn spec() -> bool { true }\n",
    )
    .expect("spec file should be written");
    fs::write(
        repo_root.join("src/fail.rs"),
        "fn fail() -> bool { false }\n",
    )
    .expect("fail file should be written");
    fs::write(repo_root.join("src/block.rs"), "fn block() {}\n")
        .expect("block file should be written");
    write_claim_file(repo_root, "REQ-auth-001", "Healthy");
    write_claim_file(repo_root, "REQ-auth-002", "Needs test");
    write_claim_file(repo_root, "REQ-auth-003", "Needs code");
    write_claim_file(repo_root, "REQ-auth-004", "Needs spec");
    write_claim_file(repo_root, "REQ-auth-005", "Contradicted");
    write_claim_file(repo_root, "REQ-auth-006", "Blocked");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::healthy",
            &["src/healthy.rs"],
        ))
        .expect("healthy evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-002",
            Verdict::Pass,
            "auth::stale",
            &["src/stale.rs"],
        ))
        .expect("stale evidence should append");
    fs::write(
        repo_root.join("src/stale.rs"),
        "fn stale() -> bool { false }\n",
    )
    .expect("stale file should change");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000003",
            "REQ-auth-004",
            Verdict::Pass,
            "auth::needs_spec",
            &["src/spec.rs"],
        ))
        .expect("needs-spec evidence should append");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000001",
            "REQ-auth-004",
            PatchState::Pending,
        ))
        .expect("pending patch should store");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000004",
            "REQ-auth-005",
            Verdict::Fail,
            "auth::fail",
            &["src/fail.rs"],
        ))
        .expect("fail evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000005",
            "REQ-auth-006",
            Verdict::Unknown,
            "auth::blocked",
            &["src/block.rs"],
        ))
        .expect("blocked evidence should append");

    let claims = triad.list_claims().expect("claim list should resolve");
    let status = triad.status(None).expect("status should resolve");

    assert_eq!(status.claims, claims);
    assert_eq!(
        status.summary,
        triad_core::StatusSummary {
            healthy: 1,
            needs_code: 1,
            needs_test: 1,
            needs_spec: 1,
            contradicted: 1,
            blocked: 1,
        }
    );
    assert_eq!(
        status
            .claims
            .iter()
            .map(|claim| claim.claim_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "REQ-auth-001",
            "REQ-auth-002",
            "REQ-auth-003",
            "REQ-auth-004",
            "REQ-auth-005",
            "REQ-auth-006",
        ]
    );
}

#[test]
fn status_rollup_can_filter_single_claim() {
    let temp = TestDir::new("status-rollup-single");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/spec.rs"),
        "fn spec() -> bool { true }\n",
    )
    .expect("spec file should be written");
    write_claim_file(repo_root, "REQ-auth-001", "Filtered");
    write_claim_file(repo_root, "REQ-auth-002", "Other");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::needs_spec",
            &["src/spec.rs"],
        ))
        .expect("needs-spec evidence should append");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000001",
            "REQ-auth-001",
            PatchState::Pending,
        ))
        .expect("pending patch should store");

    let report = triad
        .status(Some(
            &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        ))
        .expect("filtered status should resolve");

    assert_eq!(
        report,
        triad_core::StatusReport {
            summary: triad_core::StatusSummary {
                healthy: 0,
                needs_code: 0,
                needs_test: 0,
                needs_spec: 1,
                contradicted: 0,
                blocked: 0,
            },
            claims: vec![triad_core::ClaimSummary {
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                title: "Filtered".to_string(),
                status: DriftStatus::NeedsSpec,
                revision: parse_claim_file(&utf8(repo_root.join("spec/claims/REQ-auth-001.md")))
                    .expect("claim should parse")
                    .revision,
                pending_patch_id: Some(
                    PatchId::new("PATCH-000001").expect("patch id should parse"),
                ),
            }],
        }
    );
}

#[test]
fn malformed_claim_status_ignores_invalid_claim_and_collects_diagnostics() {
    let temp = TestDir::new("malformed-claim-status");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let claim_dir = repo_root.join("spec/claims");

    triad.init_scaffold(false).expect("scaffold should succeed");
    write_claim_file(repo_root, "REQ-auth-002", "Valid claim");
    fs::write(
        claim_dir.join("REQ-auth-001.md"),
        "\
# REQ-auth-001 Broken claim

## Claim
Broken

## Examples
- broken
",
    )
    .expect("malformed claim should be written");

    let status = triad.status(None).expect("status should resolve");
    let diagnostics = triad
        .claim_load_diagnostics()
        .expect("diagnostics should resolve");

    assert_eq!(status.claims.len(), 1);
    assert_eq!(status.claims[0].claim_id.as_str(), "REQ-auth-002");
    assert_eq!(status.summary.needs_code, 1);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains(
        "malformed claim REQ-auth-001: parse error: missing required section Invariants"
    ));
}

#[test]
fn malformed_claim_next_ignores_invalid_claim_when_valid_claim_exists() {
    let temp = TestDir::new("malformed-claim-next");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let claim_dir = repo_root.join("spec/claims");

    triad.init_scaffold(false).expect("scaffold should succeed");
    write_claim_file(repo_root, "REQ-auth-002", "Valid claim");
    fs::write(
        claim_dir.join("REQ-auth-001.md"),
        "\
# REQ-auth-001 Broken claim

## Claim
Broken
",
    )
    .expect("malformed claim should be written");

    let next = triad.next_claim().expect("next should resolve");

    assert_eq!(next.claim_id.as_str(), "REQ-auth-002");
    assert_eq!(next.status, DriftStatus::NeedsCode);
    assert_eq!(next.next_action, NextAction::Work);
}

#[test]
fn malformed_claim_filtered_status_reports_specific_claim_and_cause() {
    let temp = TestDir::new("malformed-claim-filtered-status");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let claim_dir = repo_root.join("spec/claims");

    triad.init_scaffold(false).expect("scaffold should succeed");
    write_claim_file(repo_root, "REQ-auth-002", "Valid claim");
    fs::write(
        claim_dir.join("REQ-auth-001.md"),
        "\
# REQ-auth-001 Broken claim

## Claim
Broken
",
    )
    .expect("malformed claim should be written");

    let error = triad
        .status(Some(
            &ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        ))
        .expect_err("filtered malformed claim should report a specific error");

    assert!(
        error
            .to_string()
            .contains("invalid state: malformed claim REQ-auth-001: parse error:")
    );
    assert!(error.to_string().contains("REQ-auth-001.md"));
}

#[test]
fn selector_resolution_prefers_latest_run_record_selectors() {
    let temp = TestDir::new("selector-resolution-run-record");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "older".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec!["auth::old".to_string(), "auth::shared".to_string()],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("first run record should store");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000002").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "newer".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec![
                    "auth::new".to_string(),
                    "auth::shared".to_string(),
                    "auth::new".to_string(),
                ],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-2",
            &BTreeMap::new(),
        )
        .expect("second run record should store");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000003").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-002").expect("claim id should parse"),
                summary: "other claim".to_string(),
                changed_paths: vec!["src/other.rs".to_string()],
                suggested_test_selectors: vec!["other::selector".to_string()],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-3",
            &BTreeMap::new(),
        )
        .expect("other run record should store");

    let selectors = triad
        .resolve_targeted_selectors(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("selectors should resolve");

    assert_eq!(selectors, vec!["auth::new", "auth::shared"]);
}

#[test]
fn selector_resolution_falls_back_to_latest_fresh_evidence_selectors() {
    let temp = TestDir::new("selector-resolution-evidence");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/pass.rs"),
        "fn pass() -> bool { true }\n",
    )
    .expect("pass file should be written");
    fs::write(
        repo_root.join("src/fail.rs"),
        "fn fail() -> bool { false }\n",
    )
    .expect("fail file should be written");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::pass_selector",
            &["src/pass.rs"],
        ))
        .expect("pass evidence should append");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000002",
            "REQ-auth-001",
            Verdict::Fail,
            "auth::fail_selector",
            &["src/fail.rs"],
        ))
        .expect("fail evidence should append");

    let selectors = triad
        .resolve_targeted_selectors(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("selectors should resolve");

    assert_eq!(
        selectors,
        vec!["auth::fail_selector", "auth::pass_selector"]
    );
}

#[test]
fn selector_resolution_ignores_stale_evidence_and_empty_latest_run_record() {
    let temp = TestDir::new("selector-resolution-stale");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let auth_path = repo_root.join("src/auth.rs");
    fs::create_dir_all(auth_path.parent().expect("parent should exist"))
        .expect("src dir should exist");
    fs::write(&auth_path, "fn auth() -> bool { true }\n").expect("auth file should be written");

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "no selectors".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec![],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("run record should store");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::stale_selector",
            &["src/auth.rs"],
        ))
        .expect("evidence should append");
    fs::write(&auth_path, "fn auth() -> bool { false }\n").expect("auth file should change");

    let selectors = triad
        .resolve_targeted_selectors(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("selectors should resolve");

    assert!(selectors.is_empty());
}

#[test]
fn verify_layer_mapping_uses_targeted_selectors_before_workspace() {
    let temp = TestDir::new("verify-layer-mapping-targeted");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "targeted selectors".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec![
                    "auth::contract".to_string(),
                    "auth::unit".to_string(),
                ],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("run record should store");

    let plans = triad
        .plan_verify_commands(&VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![VerifyLayer::Unit, VerifyLayer::Contract],
            full_workspace: false,
        })
        .expect("verify plan should resolve");

    assert_eq!(
        plans,
        vec![
            VerifyCommandPlan {
                layer: VerifyLayer::Unit,
                command: "cargo test --lib auth::contract".to_string(),
                targeted: true,
            },
            VerifyCommandPlan {
                layer: VerifyLayer::Unit,
                command: "cargo test --lib auth::unit".to_string(),
                targeted: true,
            },
            VerifyCommandPlan {
                layer: VerifyLayer::Contract,
                command: "cargo test auth::contract".to_string(),
                targeted: true,
            },
            VerifyCommandPlan {
                layer: VerifyLayer::Contract,
                command: "cargo test auth::unit".to_string(),
                targeted: true,
            },
        ]
    );
}

#[test]
fn verify_layer_mapping_maps_default_layers_and_probe_separately() {
    let temp = TestDir::new("verify-layer-mapping-workspace");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad.init_scaffold(false).expect("scaffold should succeed");

    let default_only = triad
        .plan_verify_commands(&VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![
                VerifyLayer::Unit,
                VerifyLayer::Contract,
                VerifyLayer::Integration,
            ],
            full_workspace: true,
        })
        .expect("default verify plan should resolve");

    assert_eq!(
        default_only,
        vec![
            VerifyCommandPlan {
                layer: VerifyLayer::Unit,
                command: "cargo test --workspace --lib".to_string(),
                targeted: false,
            },
            VerifyCommandPlan {
                layer: VerifyLayer::Contract,
                command: "cargo test --workspace".to_string(),
                targeted: false,
            },
            VerifyCommandPlan {
                layer: VerifyLayer::Integration,
                command: "cargo test --workspace --tests".to_string(),
                targeted: false,
            },
        ]
    );

    let with_probe = triad
        .plan_verify_commands(&VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![
                VerifyLayer::Unit,
                VerifyLayer::Contract,
                VerifyLayer::Integration,
                VerifyLayer::Probe,
            ],
            full_workspace: true,
        })
        .expect("probe verify plan should resolve");

    assert_eq!(
        with_probe.last(),
        Some(&VerifyCommandPlan {
            layer: VerifyLayer::Probe,
            command: "cargo test --workspace --tests -- --ignored".to_string(),
            targeted: false,
        })
    );
    assert_eq!(with_probe.len(), 4);
}

#[test]
fn verify_layer_mapping_falls_back_to_workspace_when_no_selector_exists() {
    let temp = TestDir::new("verify-layer-mapping-fallback");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad.init_scaffold(false).expect("scaffold should succeed");

    let plans = triad
        .plan_verify_commands(&VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![VerifyLayer::Integration],
            full_workspace: false,
        })
        .expect("verify plan should resolve");

    assert_eq!(
        plans,
        vec![VerifyCommandPlan {
            layer: VerifyLayer::Integration,
            command: "cargo test --workspace --tests".to_string(),
            targeted: false,
        }]
    );
}

#[test]
fn command_runner_supports_fake_runner_stubs() {
    let temp = TestDir::new("command-runner-fake");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "targeted selectors".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec!["auth::unit".to_string()],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("run record should store");

    let request = VerifyRequest {
        claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        layers: vec![VerifyLayer::Unit, VerifyLayer::Integration],
        full_workspace: false,
    };
    let fake = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --lib auth::unit".to_string(), 0),
        ("cargo test --tests auth::unit".to_string(), 7),
    ]));

    let executions = execute_verify_commands_with_runner(&triad, &request, &fake)
        .expect("fake runner should execute planned commands");

    assert_eq!(
        fake.seen(),
        vec![
            VerifyCommandPlan {
                layer: VerifyLayer::Unit,
                command: "cargo test --lib auth::unit".to_string(),
                targeted: true,
            },
            VerifyCommandPlan {
                layer: VerifyLayer::Integration,
                command: "cargo test --tests auth::unit".to_string(),
                targeted: true,
            },
        ]
    );
    assert_eq!(
        executions,
        vec![
            VerifyCommandExecution {
                plan: VerifyCommandPlan {
                    layer: VerifyLayer::Unit,
                    command: "cargo test --lib auth::unit".to_string(),
                    targeted: true,
                },
                exit_code: 0,
                success: true,
            },
            VerifyCommandExecution {
                plan: VerifyCommandPlan {
                    layer: VerifyLayer::Integration,
                    command: "cargo test --tests auth::unit".to_string(),
                    targeted: true,
                },
                exit_code: 7,
                success: false,
            },
        ]
    );
}

#[test]
fn command_runner_process_runner_executes_shell_commands() {
    let runner = ProcessCommandRunner;

    let success = runner
        .run(&VerifyCommandPlan {
            layer: VerifyLayer::Unit,
            command: "true".to_string(),
            targeted: false,
        })
        .expect("true should exit successfully");
    let failure = runner
        .run(&VerifyCommandPlan {
            layer: VerifyLayer::Unit,
            command: "false".to_string(),
            targeted: false,
        })
        .expect("false should still return an exit code");

    assert_eq!(success, 0);
    assert_ne!(failure, 0);
}

#[test]
fn full_workspace_verify_ignores_targeted_selectors_when_flag_is_true() {
    let temp = TestDir::new("full-workspace-verify-workspace");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_claim_file(repo_root, "REQ-auth-001", "Login");
    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "targeted selectors exist".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec!["auth::unit".to_string()],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("run record should store");
    let fake = FakeCommandRunner::new(BTreeMap::from([(
        "cargo test --workspace --lib".to_string(),
        0,
    )]));

    let report = verify_claim_with_runner(
        &triad,
        VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![VerifyLayer::Unit],
            full_workspace: true,
        },
        &fake,
    )
    .expect("full workspace verify should succeed");
    let evidence_rows = triad.read_evidence().expect("evidence log should read");

    assert!(report.full_workspace);
    assert_eq!(
        fake.seen(),
        vec![VerifyCommandPlan {
            layer: VerifyLayer::Unit,
            command: "cargo test --workspace --lib".to_string(),
            targeted: false,
        }]
    );
    assert_eq!(evidence_rows.len(), 1);
    assert_eq!(evidence_rows[0].command, "cargo test --workspace --lib");
    assert_eq!(evidence_rows[0].test_selector, None);
}

#[test]
fn full_workspace_verify_preserves_targeted_path_when_flag_is_false() {
    let temp = TestDir::new("full-workspace-verify-targeted");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_claim_file(repo_root, "REQ-auth-001", "Login");
    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "targeted selectors exist".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec!["auth::unit".to_string()],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("run record should store");
    let fake = FakeCommandRunner::new(BTreeMap::from([(
        "cargo test --lib auth::unit".to_string(),
        0,
    )]));

    let report = verify_claim_with_runner(
        &triad,
        VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![VerifyLayer::Unit],
            full_workspace: false,
        },
        &fake,
    )
    .expect("targeted verify should succeed");
    let evidence_rows = triad.read_evidence().expect("evidence log should read");

    assert!(!report.full_workspace);
    assert_eq!(
        fake.seen(),
        vec![VerifyCommandPlan {
            layer: VerifyLayer::Unit,
            command: "cargo test --lib auth::unit".to_string(),
            targeted: true,
        }]
    );
    assert_eq!(evidence_rows.len(), 1);
    assert_eq!(evidence_rows[0].command, "cargo test --lib auth::unit");
    assert_eq!(
        evidence_rows[0].test_selector.as_deref(),
        Some("auth::unit")
    );
}

#[test]
fn probe_opt_in_excludes_probe_from_default_request() {
    let temp = TestDir::new("probe-opt-in-default");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    let request = triad
        .default_verify_request(
            ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            false,
            true,
        )
        .expect("default verify request should resolve");
    let plans = triad
        .plan_verify_commands(&request)
        .expect("verify plans should resolve");

    assert_eq!(
        request.layers,
        vec![
            VerifyLayer::Unit,
            VerifyLayer::Contract,
            VerifyLayer::Integration,
        ]
    );
    assert!(plans.iter().all(|plan| plan.layer != VerifyLayer::Probe));
    assert!(
        plans
            .iter()
            .all(|plan| !plan.command.contains("-- --ignored"))
    );
}

#[test]
fn probe_opt_in_appends_probe_only_when_requested() {
    let temp = TestDir::new("probe-opt-in-enabled");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    let request = triad
        .default_verify_request(
            ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            true,
            true,
        )
        .expect("probe verify request should resolve");
    let plans = triad
        .plan_verify_commands(&request)
        .expect("verify plans should resolve");

    assert_eq!(
        request.layers,
        vec![
            VerifyLayer::Unit,
            VerifyLayer::Contract,
            VerifyLayer::Integration,
            VerifyLayer::Probe,
        ]
    );
    assert_eq!(
        plans
            .iter()
            .filter(|plan| plan.layer == VerifyLayer::Probe)
            .count(),
        1
    );
    assert!(
        plans
            .iter()
            .any(|plan| plan.command == "cargo test --workspace --tests -- --ignored")
    );
}

#[test]
fn probe_opt_in_rejects_probe_in_default_config_layers() {
    let temp = TestDir::new("probe-opt-in-invalid-config");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.verify.default_layers.push("probe".to_string());
    let triad = LocalTriad::new(config);

    let error = triad
        .default_verify_request(
            ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            false,
            false,
        )
        .expect_err("probe in default config should fail");

    assert_eq!(
        error.to_string(),
        "config error: invalid config verify.default_layers: probe must be enabled only via --with-probe"
    );
}

#[test]
fn verification_contract_verified_evidence_becomes_stale_after_covered_file_changes() {
    let temp = TestDir::new("verification-contract-stale");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let auth_path = repo_root.join("src/auth.rs");

    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(auth_path.parent().expect("parent should exist"))
        .expect("src dir should exist");
    fs::write(&auth_path, "fn auth() -> bool { true }\n").expect("auth file should be written");
    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "auth path changed".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec!["auth::unit".to_string()],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("run record should store");

    let report = verify_claim_with_runner(
        &triad,
        VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![VerifyLayer::Unit],
            full_workspace: false,
        },
        &FakeCommandRunner::new(BTreeMap::from([(
            "cargo test --lib auth::unit".to_string(),
            0,
        )])),
    )
    .expect("verify should append evidence");
    let evidence_rows = triad.read_evidence().expect("evidence log should read");

    assert_eq!(report.status_after_verify, DriftStatus::Healthy);
    assert_eq!(evidence_rows.len(), 1);
    assert_eq!(
        evidence_rows[0].covered_paths,
        vec![Utf8PathBuf::from("src/auth.rs")]
    );
    assert!(
        evidence_rows[0]
            .covered_digests
            .get(camino::Utf8Path::new("src/auth.rs"))
            .expect("digest should exist")
            .starts_with("sha256:")
    );

    fs::write(&auth_path, "fn auth() -> bool { false }\n").expect("auth file should change");

    let drift = triad
        .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("drift should resolve");

    assert_eq!(drift.status, DriftStatus::NeedsTest);
    assert!(drift.fresh_evidence_ids.is_empty());
}

#[test]
fn verification_contract_failing_verify_writes_failure_evidence_with_coverage() {
    let temp = TestDir::new("verification-contract-fail");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let auth_path = repo_root.join("src/auth.rs");

    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::create_dir_all(auth_path.parent().expect("parent should exist"))
        .expect("src dir should exist");
    fs::write(&auth_path, "fn auth() -> bool { false }\n").expect("auth file should be written");
    triad.init_scaffold(false).expect("scaffold should succeed");
    triad
        .store_run_record(
            &RunClaimReport {
                run_id: RunId::new("RUN-000001").expect("run id should parse"),
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                summary: "contract path changed".to_string(),
                changed_paths: vec!["src/auth.rs".to_string()],
                suggested_test_selectors: vec!["auth::contract".to_string()],
                blocked_actions: vec![],
                needs_patch: false,
            },
            "fp-1",
            &BTreeMap::new(),
        )
        .expect("run record should store");

    let report = verify_claim_with_runner(
        &triad,
        VerifyRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            layers: vec![VerifyLayer::Contract],
            full_workspace: false,
        },
        &FakeCommandRunner::new(BTreeMap::from([(
            "cargo test auth::contract".to_string(),
            9,
        )])),
    )
    .expect("verify should append failing evidence");
    let evidence_rows = triad.read_evidence().expect("evidence log should read");

    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(report.status_after_verify, DriftStatus::Contradicted);
    assert_eq!(evidence_rows.len(), 1);
    assert_eq!(evidence_rows[0].kind, EvidenceKind::Contract);
    assert_eq!(evidence_rows[0].verdict, Verdict::Fail);
    assert_eq!(evidence_rows[0].command, "cargo test auth::contract");
    assert_eq!(
        evidence_rows[0].test_selector.as_deref(),
        Some("auth::contract")
    );
    assert_eq!(
        evidence_rows[0].covered_paths,
        vec![Utf8PathBuf::from("src/auth.rs")]
    );
    assert!(
        triad
            .evidence_is_fresh(&evidence_rows[0])
            .expect("freshness should evaluate")
    );
}

#[test]
fn verify_writes_evidence_and_updates_drift_to_healthy() {
    let temp = TestDir::new("verify-writes-evidence-pass");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_claim_file(repo_root, "REQ-auth-001", "Login");
    triad.init_scaffold(false).expect("scaffold should succeed");

    let request = VerifyRequest {
        claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        layers: vec![VerifyLayer::Unit, VerifyLayer::Integration],
        full_workspace: true,
    };
    let fake = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --workspace --lib".to_string(), 0),
        ("cargo test --workspace --tests".to_string(), 0),
    ]));

    let report = verify_claim_with_runner(&triad, request, &fake)
        .expect("verify should append passing evidence");
    let evidence_rows = triad.read_evidence().expect("evidence log should read");

    assert_eq!(
        report,
        VerifyReport {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            verdict: Verdict::Pass,
            layers: vec![VerifyLayer::Unit, VerifyLayer::Integration],
            full_workspace: true,
            evidence_ids: vec![
                EvidenceId::new("EVID-000001").expect("evidence id should parse"),
                EvidenceId::new("EVID-000002").expect("evidence id should parse"),
            ],
            status_after_verify: DriftStatus::Healthy,
            pending_patch_id: None,
        }
    );
    assert_eq!(evidence_rows.len(), 2);
    assert_eq!(evidence_rows[0].kind, EvidenceKind::Unit);
    assert_eq!(evidence_rows[0].verdict, Verdict::Pass);
    assert_eq!(evidence_rows[0].command, "cargo test --workspace --lib");
    assert_eq!(evidence_rows[0].test_selector, None);
    assert_eq!(
        evidence_rows[0].spec_revision,
        parse_claim_file(&utf8(repo_root.join("spec/claims/REQ-auth-001.md")))
            .expect("claim should parse")
            .revision
    );
    assert!(evidence_rows[0].created_at.starts_with("unix:"));
    assert_eq!(evidence_rows[1].kind, EvidenceKind::Integration);
    assert_eq!(evidence_rows[1].verdict, Verdict::Pass);
    assert_eq!(evidence_rows[1].command, "cargo test --workspace --tests");
    assert_eq!(
        triad
            .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
            .expect("drift should resolve")
            .status,
        DriftStatus::Healthy
    );
}

#[test]
fn verify_writes_evidence_and_updates_drift_to_contradicted() {
    let temp = TestDir::new("verify-writes-evidence-fail");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_claim_file(repo_root, "REQ-auth-001", "Login");
    triad.init_scaffold(false).expect("scaffold should succeed");

    let request = VerifyRequest {
        claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        layers: vec![VerifyLayer::Unit, VerifyLayer::Contract],
        full_workspace: false,
    };
    let fake = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --workspace --lib".to_string(), 0),
        ("cargo test --workspace".to_string(), 9),
    ]));

    let report = verify_claim_with_runner(&triad, request, &fake)
        .expect("verify should append failing evidence");
    let evidence_rows = triad.read_evidence().expect("evidence log should read");

    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(report.status_after_verify, DriftStatus::Contradicted);
    assert_eq!(
        report.evidence_ids,
        vec![
            EvidenceId::new("EVID-000001").expect("evidence id should parse"),
            EvidenceId::new("EVID-000002").expect("evidence id should parse"),
        ]
    );
    assert_eq!(evidence_rows.len(), 2);
    assert_eq!(evidence_rows[0].verdict, Verdict::Pass);
    assert_eq!(evidence_rows[1].verdict, Verdict::Fail);
    assert_eq!(evidence_rows[0].kind, EvidenceKind::Unit);
    assert_eq!(evidence_rows[1].kind, EvidenceKind::Contract);
    assert_eq!(evidence_rows[0].command, "cargo test --workspace --lib");
    assert_eq!(evidence_rows[1].command, "cargo test --workspace");
    assert_eq!(
        triad
            .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
            .expect("drift should resolve")
            .status,
        DriftStatus::Contradicted
    );
}

#[test]
fn patch_store_writes_meta_and_diff_pair() {
    let temp = TestDir::new("patch-store");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    let draft = test_patch_draft("PATCH-000001");
    triad
        .store_patch_draft(&draft)
        .expect("patch draft should store");

    let json_path = repo_root.join(".triad/patches/PATCH-000001.json");
    let diff_path = repo_root.join(".triad/patches/PATCH-000001.diff");
    let meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_path).expect("meta should be readable"))
            .expect("meta json should parse");

    assert!(json_path.is_file());
    assert!(diff_path.is_file());
    assert_eq!(meta["id"], "PATCH-000001");
    assert_eq!(meta["claim_id"], "REQ-auth-001");
    assert_eq!(meta["state"], "pending");
    assert_eq!(meta["diff_path"], ".triad/patches/PATCH-000001.diff");
    assert_eq!(
        fs::read_to_string(&diff_path).expect("diff should be readable"),
        draft.unified_diff
    );
}

#[test]
fn patch_store_roundtrips_patch_draft_from_meta_and_diff() {
    let temp = TestDir::new("patch-store-read");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    let draft = test_patch_draft("PATCH-000001");
    triad
        .store_patch_draft(&draft)
        .expect("patch draft should store");

    let roundtrip = triad
        .read_patch_draft(&PatchId::new("PATCH-000001").expect("patch id should parse"))
        .expect("patch draft should read");

    assert_eq!(roundtrip.id, draft.id);
    assert_eq!(roundtrip.claim_id, draft.claim_id);
    assert_eq!(roundtrip.based_on_evidence, draft.based_on_evidence);
    assert_eq!(roundtrip.rationale, draft.rationale);
    assert_eq!(roundtrip.created_at, draft.created_at);
    assert_eq!(roundtrip.state, draft.state);
    assert_eq!(roundtrip.unified_diff, draft.unified_diff);
}

#[test]
fn patch_persistence_propose_patch_writes_meta_and_diff_pair() {
    let temp = TestDir::new("patch-persistence-propose");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login success");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let report = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should persist");

    assert_eq!(report.patch_id.as_str(), "PATCH-000001");
    assert_eq!(report.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(
        report.based_on_evidence,
        vec![EvidenceId::new("EVID-000001").expect("evidence id should parse")]
    );
    assert_eq!(report.path, ".triad/patches/PATCH-000001.diff");
    assert_eq!(
        report.reason,
        "latest run RUN-000001 marked needs_patch after fresh pass evidence EVID-000001 on src/auth.rs: Updated auth handler and tests."
    );

    let stored = triad
        .read_patch_draft(&report.patch_id)
        .expect("stored patch should read");
    assert_eq!(stored.claim_id, report.claim_id);
    assert_eq!(stored.based_on_evidence, report.based_on_evidence);
    assert_eq!(stored.rationale, report.reason);
    assert_eq!(stored.state, PatchState::Pending);
    assert!(
        stored
            .unified_diff
            .contains("--- a/spec/claims/REQ-auth-001.md\n")
    );
    assert!(
        stored
            .unified_diff
            .contains("+++ b/spec/claims/REQ-auth-001.md\n")
    );
    assert!(
        stored
            .unified_diff
            .contains("+Behavior update: Updated auth handler and tests.\n")
    );

    assert!(repo_root.join(".triad/patches/PATCH-000001.json").is_file());
    assert!(repo_root.join(".triad/patches/PATCH-000001.diff").is_file());
}

#[test]
fn patch_persistence_next_patch_id_uses_monotonic_patch_stem_sequence() {
    let temp = TestDir::new("patch-persistence-next-id");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    triad
        .store_patch_draft(&test_patch_draft("PATCH-000001"))
        .expect("first patch should store");
    triad
        .store_patch_draft(&test_patch_draft("PATCH-000003"))
        .expect("third patch should store");

    let next = triad.next_patch_id().expect("next patch id should resolve");
    assert_eq!(next.as_str(), "PATCH-000004");
}

#[test]
fn patch_persistence_rejects_duplicate_pending_patch_for_claim() {
    let temp = TestDir::new("patch-persistence-duplicate-pending");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    write_claim_file(repo_root, "REQ-auth-001", "Login success");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_patch_draft(&test_patch_draft("PATCH-000001"))
        .expect("pending patch should store");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let error = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect_err("duplicate pending patch should fail");

    assert_eq!(
        error.to_string(),
        "invalid state: pending patch already exists for REQ-auth-001: PATCH-000001"
    );
}

#[test]
fn patch_conflict_applies_pending_patch_and_marks_it_applied() {
    let temp = TestDir::new("patch-conflict-apply");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.verify.full_workspace_after_accept = false;
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login success");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let proposed = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should succeed");
    let report = triad
        .apply_patch(&proposed.patch_id)
        .expect("patch apply should succeed");
    let accepted_claim = parse_claim_file(&utf8(repo_root.join("spec/claims/REQ-auth-001.md")))
        .expect("accepted claim should parse");

    assert!(report.applied);
    assert_eq!(report.patch_id, proposed.patch_id);
    assert_eq!(report.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(report.new_revision, accepted_claim.revision);
    assert_eq!(report.followup_action, NextAction::Verify);
    assert_eq!(
        triad
            .read_patch_draft(&proposed.patch_id)
            .expect("patch draft should read")
            .state,
        PatchState::Applied
    );
    assert_eq!(
        fs::read_to_string(repo_root.join("spec/claims/REQ-auth-001.md"))
            .expect("claim file should read"),
        "\
# REQ-auth-001 Login success

## Claim
User can log in with valid credentials.

## Examples
- valid credentials -> 200 + session cookie

## Invariants
- password plaintext never appears in logs

## Notes
- MFA is out of scope
Behavior update: Updated auth handler and tests.
"
    );
}

#[test]
fn patch_conflict_rejects_when_claim_file_changed_after_patch_generation() {
    let temp = TestDir::new("patch-conflict-mismatch");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login success");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let proposed = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should succeed");
    fs::write(
        repo_root.join("spec/claims/REQ-auth-001.md"),
        "\
# REQ-auth-001 Login success

## Claim
User can log in with a magic link.

## Examples
- valid credentials -> 200 + session cookie

## Invariants
- password plaintext never appears in logs

## Notes
MFA is out of scope
",
    )
    .expect("claim file should be overwritten");

    let error = triad
        .apply_patch(&proposed.patch_id)
        .expect_err("changed claim file should conflict");

    assert_eq!(
        error.to_string(),
        "patch conflict: PATCH-000001: claim file no longer matches spec/claims/REQ-auth-001.md"
    );
    assert_eq!(
        triad
            .read_patch_draft(&proposed.patch_id)
            .expect("patch draft should still read")
            .state,
        PatchState::Pending
    );
}

#[test]
fn accept_flow_recomputes_revision_and_requests_followup_verify_when_disabled() {
    let temp = TestDir::new("accept-flow-no-post-verify");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.verify.full_workspace_after_accept = false;
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login success");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let proposed = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should succeed");
    let fake = FakeCommandRunner::new(BTreeMap::new());
    let report = apply_patch_with_runner(&triad, &proposed.patch_id, &fake)
        .expect("patch apply should succeed");

    let accepted_claim = parse_claim_file(&utf8(repo_root.join("spec/claims/REQ-auth-001.md")))
        .expect("accepted claim should parse");
    assert_eq!(report.new_revision, accepted_claim.revision);
    assert_eq!(report.followup_action, NextAction::Verify);
    assert!(fake.seen().is_empty());
}

#[test]
fn accept_flow_runs_optional_full_workspace_verify_and_returns_status_followup() {
    let temp = TestDir::new("accept-flow-post-verify");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    write_claim_file(repo_root, "REQ-auth-001", "Login success");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let proposed = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should succeed");
    let fake = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --workspace --lib".to_string(), 0),
        ("cargo test --workspace".to_string(), 0),
        ("cargo test --workspace --tests".to_string(), 0),
    ]));
    let report = apply_patch_with_runner(&triad, &proposed.patch_id, &fake)
        .expect("patch apply should succeed");

    let accepted_claim = parse_claim_file(&utf8(repo_root.join("spec/claims/REQ-auth-001.md")))
        .expect("accepted claim should parse");
    let evidence_rows = triad.read_evidence().expect("evidence should read");

    assert_eq!(report.new_revision, accepted_claim.revision);
    assert_eq!(report.followup_action, NextAction::Status);
    assert_eq!(
        fake.seen()
            .into_iter()
            .map(|plan| plan.command)
            .collect::<Vec<_>>(),
        vec![
            "cargo test --workspace --lib".to_string(),
            "cargo test --workspace".to_string(),
            "cargo test --workspace --tests".to_string(),
        ]
    );
    assert_eq!(evidence_rows.len(), 4);
    assert_eq!(evidence_rows[1].spec_revision, accepted_claim.revision);
    assert_eq!(evidence_rows[2].spec_revision, accepted_claim.revision);
    assert_eq!(evidence_rows[3].spec_revision, accepted_claim.revision);
    assert_eq!(
        triad
            .detect_drift(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
            .expect("drift should resolve")
            .status,
        DriftStatus::Healthy
    );
}

#[test]
fn e2e_happy_path_fixture_ratchets_single_claim_to_healthy() {
    let temp = TestDir::new("e2e-happy-path");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/auth.rs"),
        "pub fn login() -> bool { true }\n",
    )
    .expect("source file should be written");

    let next_before = triad.next_claim().expect("next claim should resolve");
    assert_eq!(next_before.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(next_before.status, DriftStatus::NeedsCode);
    assert_eq!(next_before.next_action, NextAction::Work);

    let work_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "implemented login handler",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );
    let run_report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: next_before.claim_id.clone(),
            dry_run: false,
            model: None,
            effort: None,
        },
        &work_runner,
    )
    .expect("work run should succeed");

    assert_eq!(work_runner.calls().len(), 1);
    assert_eq!(run_report.run_id.as_str(), "RUN-000001");
    assert_eq!(run_report.claim_id, next_before.claim_id);
    assert_eq!(
        run_report.suggested_test_selectors,
        vec!["auth::login_success".to_string()]
    );

    let verify_request = triad
        .default_verify_request(next_before.claim_id.clone(), false, false)
        .expect("default verify request should build");
    let verify_runner = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --lib auth::login_success".to_string(), 0),
        ("cargo test auth::login_success".to_string(), 0),
        ("cargo test --tests auth::login_success".to_string(), 0),
    ]));
    let verify_report = verify_claim_with_runner(&triad, verify_request, &verify_runner)
        .expect("verify should succeed");

    assert_eq!(verify_report.claim_id, next_before.claim_id);
    assert_eq!(verify_report.verdict, Verdict::Pass);
    assert_eq!(verify_report.status_after_verify, DriftStatus::Healthy);
    assert!(verify_report.pending_patch_id.is_none());
    assert_eq!(verify_report.evidence_ids.len(), 3);
    assert_eq!(
        verify_runner
            .seen()
            .into_iter()
            .map(|plan| plan.command)
            .collect::<Vec<_>>(),
        vec![
            "cargo test --lib auth::login_success".to_string(),
            "cargo test auth::login_success".to_string(),
            "cargo test --tests auth::login_success".to_string(),
        ]
    );

    let drift = triad
        .detect_drift(&next_before.claim_id)
        .expect("drift should resolve after verify");
    assert_eq!(drift.status, DriftStatus::Healthy);
    assert!(drift.pending_patch_id.is_none());

    let status = triad
        .status(Some(&next_before.claim_id))
        .expect("status should resolve");
    assert_eq!(status.summary.healthy, 1);
    assert_eq!(status.summary.needs_code, 0);
    assert_eq!(status.summary.needs_test, 0);
    assert_eq!(status.claims.len(), 1);
    assert_eq!(status.claims[0].status, DriftStatus::Healthy);

    let next_after = triad.next_claim().expect("healthy fallback should resolve");
    assert_eq!(next_after.claim_id, next_before.claim_id);
    assert_eq!(next_after.status, DriftStatus::Healthy);
    assert_eq!(next_after.next_action, NextAction::Status);
}

#[test]
fn e2e_contradicted_fixture_marks_single_claim_as_contradicted() {
    let temp = TestDir::new("e2e-contradicted");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/auth.rs"),
        "pub fn login() -> bool { false }\n",
    )
    .expect("source file should be written");

    let next_before = triad.next_claim().expect("next claim should resolve");
    assert_eq!(next_before.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(next_before.status, DriftStatus::NeedsCode);
    assert_eq!(next_before.next_action, NextAction::Work);

    let work_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "implemented login handler with a failing branch",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );
    let run_report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: next_before.claim_id.clone(),
            dry_run: false,
            model: None,
            effort: None,
        },
        &work_runner,
    )
    .expect("work run should succeed");

    assert_eq!(work_runner.calls().len(), 1);
    assert_eq!(run_report.run_id.as_str(), "RUN-000001");
    assert_eq!(run_report.claim_id, next_before.claim_id);

    let verify_request = triad
        .default_verify_request(next_before.claim_id.clone(), false, false)
        .expect("default verify request should build");
    let verify_runner = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --lib auth::login_success".to_string(), 0),
        ("cargo test auth::login_success".to_string(), 9),
        ("cargo test --tests auth::login_success".to_string(), 0),
    ]));
    let verify_report = verify_claim_with_runner(&triad, verify_request, &verify_runner)
        .expect("verify should complete with failing evidence");

    assert_eq!(verify_report.claim_id, next_before.claim_id);
    assert_eq!(verify_report.verdict, Verdict::Fail);
    assert_eq!(verify_report.status_after_verify, DriftStatus::Contradicted);
    assert!(verify_report.pending_patch_id.is_none());
    assert_eq!(verify_report.evidence_ids.len(), 3);

    let drift = triad
        .detect_drift(&next_before.claim_id)
        .expect("drift should resolve after failing verify");
    assert_eq!(drift.status, DriftStatus::Contradicted);
    assert_eq!(drift.reasons, vec!["latest fresh evidence is failing"]);
    assert!(drift.pending_patch_id.is_none());

    let status = triad
        .status(Some(&next_before.claim_id))
        .expect("status should resolve");
    assert_eq!(status.summary.contradicted, 1);
    assert_eq!(status.summary.healthy, 0);
    assert_eq!(status.claims.len(), 1);
    assert_eq!(status.claims[0].status, DriftStatus::Contradicted);

    let next_after = triad
        .next_claim()
        .expect("contradicted next should resolve");
    assert_eq!(next_after.claim_id, next_before.claim_id);
    assert_eq!(next_after.status, DriftStatus::Contradicted);
    assert_eq!(next_after.next_action, NextAction::Work);
}

#[test]
fn e2e_blocked_fixture_reports_live_guardrail_violation_as_runtime_blocked() {
    let temp = TestDir::new("e2e-blocked");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");

    let next_before = triad.next_claim().expect("next claim should resolve");
    assert_eq!(next_before.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(next_before.status, DriftStatus::NeedsCode);
    assert_eq!(next_before.next_action, NextAction::Work);

    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");
    let work_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "attempted forbidden push",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": [],
"blocked_actions": ["git push"],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );

    let blocked = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: next_before.claim_id.clone(),
            dry_run: false,
            model: None,
            effort: None,
        },
        &work_runner,
    )
    .expect_err("git push should be blocked in live run path");

    assert_eq!(
        blocked.to_string(),
        "runtime blocked: git push blocked by work guardrails"
    );
    assert_eq!(blocked.kind(), TriadErrorKind::RuntimeBlocked);
    assert_eq!(
        triad
            .next_run_id()
            .expect("blocked work must not persist a run record")
            .as_str(),
        "RUN-000001"
    );
}

#[test]
fn e2e_stale_evidence_fixture_demotes_verified_claim_to_needs_test() {
    let temp = TestDir::new("e2e-stale-evidence");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    let auth_path = repo_root.join("src/auth.rs");
    fs::write(&auth_path, "pub fn login() -> bool { true }\n")
        .expect("source file should be written");

    let next_before = triad.next_claim().expect("next claim should resolve");
    assert_eq!(next_before.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(next_before.status, DriftStatus::NeedsCode);
    assert_eq!(next_before.next_action, NextAction::Work);

    let work_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "implemented login handler",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": false
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );
    let run_report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: next_before.claim_id.clone(),
            dry_run: false,
            model: None,
            effort: None,
        },
        &work_runner,
    )
    .expect("work run should succeed");

    assert_eq!(work_runner.calls().len(), 1);
    assert_eq!(run_report.run_id.as_str(), "RUN-000001");
    assert_eq!(run_report.claim_id, next_before.claim_id);
    assert_eq!(
        run_report.suggested_test_selectors,
        vec!["auth::login_success".to_string()]
    );

    let verify_request = triad
        .default_verify_request(next_before.claim_id.clone(), false, false)
        .expect("default verify request should build");
    let verify_runner = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --lib auth::login_success".to_string(), 0),
        ("cargo test auth::login_success".to_string(), 0),
        ("cargo test --tests auth::login_success".to_string(), 0),
    ]));
    let verify_report = verify_claim_with_runner(&triad, verify_request, &verify_runner)
        .expect("verify should succeed");

    assert_eq!(verify_report.claim_id, next_before.claim_id);
    assert_eq!(verify_report.verdict, Verdict::Pass);
    assert_eq!(verify_report.status_after_verify, DriftStatus::Healthy);
    assert_eq!(verify_report.evidence_ids.len(), 3);

    fs::write(&auth_path, "pub fn login() -> bool { false }\n").expect("source file should change");

    let drift = triad
        .detect_drift(&next_before.claim_id)
        .expect("drift should resolve after stale change");
    assert_eq!(drift.status, DriftStatus::NeedsTest);
    assert_eq!(
        drift.reasons,
        vec!["no fresh evidence exists and implementation paths were previously observed"]
    );
    assert!(drift.pending_patch_id.is_none());
    assert!(drift.fresh_evidence_ids.is_empty());

    let status = triad
        .status(Some(&next_before.claim_id))
        .expect("status should resolve");
    assert_eq!(status.summary.needs_test, 1);
    assert_eq!(status.summary.healthy, 0);
    assert_eq!(status.claims.len(), 1);
    assert_eq!(status.claims[0].status, DriftStatus::NeedsTest);

    let next_after = triad.next_claim().expect("stale next should resolve");
    assert_eq!(next_after.claim_id, next_before.claim_id);
    assert_eq!(next_after.status, DriftStatus::NeedsTest);
    assert_eq!(next_after.next_action, NextAction::Verify);
    assert_eq!(
        next_after.reason,
        "no fresh evidence exists and implementation paths were previously observed"
    );
}

#[test]
fn e2e_needs_spec_fixture_exposes_pending_patch_after_fresh_pass() {
    let temp = TestDir::new("e2e-needs-spec");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(
        repo_root.join("src/auth.rs"),
        "pub fn login() -> bool { true }\n",
    )
    .expect("source file should be written");

    let next_before = triad.next_claim().expect("next claim should resolve");
    assert_eq!(next_before.claim_id.as_str(), "REQ-auth-001");
    assert_eq!(next_before.status, DriftStatus::NeedsCode);
    assert_eq!(next_before.next_action, NextAction::Work);

    let work_runner = FakeProcessRunner::codex_success(
        r#"{
  "schema_version": 1,
  "ok": true,
  "command": "run",
  "data": {
"claim_id": "REQ-auth-001",
"summary": "implemented login handler and identified spec drift",
"changed_paths": ["src/auth.rs"],
"suggested_test_selectors": ["auth::login_success"],
"blocked_actions": [],
"needs_patch": true
  },
  "diagnostics": []
}"#,
        Vec::new(),
    );
    let run_report = run_claim_with_backend_adapter(
        &triad,
        RunClaimRequest {
            claim_id: next_before.claim_id.clone(),
            dry_run: false,
            model: None,
            effort: None,
        },
        &work_runner,
    )
    .expect("work run should succeed");

    assert_eq!(work_runner.calls().len(), 1);
    assert_eq!(run_report.run_id.as_str(), "RUN-000001");
    assert_eq!(run_report.claim_id, next_before.claim_id);
    assert!(run_report.needs_patch);

    let verify_request = triad
        .default_verify_request(next_before.claim_id.clone(), false, false)
        .expect("default verify request should build");
    let verify_runner = FakeCommandRunner::new(BTreeMap::from([
        ("cargo test --lib auth::login_success".to_string(), 0),
        ("cargo test auth::login_success".to_string(), 0),
        ("cargo test --tests auth::login_success".to_string(), 0),
    ]));
    let verify_report = verify_claim_with_runner(&triad, verify_request, &verify_runner)
        .expect("verify should succeed");

    assert_eq!(verify_report.claim_id, next_before.claim_id);
    assert_eq!(verify_report.verdict, Verdict::Pass);
    assert_eq!(verify_report.status_after_verify, DriftStatus::Healthy);
    assert!(verify_report.pending_patch_id.is_none());
    assert_eq!(verify_report.evidence_ids.len(), 3);

    let proposed = triad
        .propose_patch(&next_before.claim_id)
        .expect("patch proposal should succeed");

    assert_eq!(proposed.claim_id, next_before.claim_id);
    assert_eq!(proposed.patch_id.as_str(), "PATCH-000001");
    assert_eq!(proposed.based_on_evidence.len(), 1);

    let drift = triad
        .detect_drift(&next_before.claim_id)
        .expect("drift should resolve after pending patch");
    assert_eq!(drift.status, DriftStatus::NeedsSpec);
    assert_eq!(
        drift.reasons,
        vec!["fresh pass evidence exists and a pending patch is present"]
    );
    assert_eq!(drift.pending_patch_id, Some(proposed.patch_id.clone()));
    assert_eq!(drift.fresh_evidence_ids.len(), 1);

    let status = triad
        .status(Some(&next_before.claim_id))
        .expect("status should resolve");
    assert_eq!(status.summary.needs_spec, 1);
    assert_eq!(status.summary.healthy, 0);
    assert_eq!(status.claims.len(), 1);
    assert_eq!(status.claims[0].status, DriftStatus::NeedsSpec);
    assert_eq!(
        status.claims[0].pending_patch_id,
        Some(proposed.patch_id.clone())
    );

    let next_after = triad.next_claim().expect("needs-spec next should resolve");
    assert_eq!(next_after.claim_id, next_before.claim_id);
    assert_eq!(next_after.status, DriftStatus::NeedsSpec);
    assert_eq!(next_after.next_action, NextAction::Accept);
    assert_eq!(
        next_after.reason,
        "fresh pass evidence exists and a pending patch is present"
    );
}

#[test]
fn patch_golden_propose_emits_exact_diff_for_repo_fixture() {
    let temp = TestDir::new("patch-golden-propose");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.verify.full_workspace_after_accept = false;
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let proposal = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should succeed");
    let draft = triad
        .read_patch_draft(&proposal.patch_id)
        .expect("patch draft should read");

    assert_eq!(
        draft.unified_diff,
        "\
--- a/spec/claims/REQ-auth-001.md
+++ b/spec/claims/REQ-auth-001.md
@@ -16,0 +17 @@
+Behavior update: Updated auth handler and tests.
"
    );
}

#[test]
fn patch_golden_apply_emits_exact_repo_fixture_content() {
    let temp = TestDir::new("patch-golden-apply");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.verify.full_workspace_after_accept = false;
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let proposal = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should succeed");
    triad
        .apply_patch(&proposal.patch_id)
        .expect("patch apply should succeed");

    assert_eq!(
        fs::read_to_string(repo_root.join("spec/claims/REQ-auth-001.md"))
            .expect("claim file should read"),
        "\
# REQ-auth-001 Login success

## Claim
사용자는 유효한 이메일/비밀번호 조합으로 로그인할 수 있어야 한다.

## Examples
- valid credentials -> 200 + session cookie
- wrong password -> 401
- deleted user -> 404

## Invariants
- 비밀번호 원문은 로그에 남지 않는다.
- 실패 응답은 계정 존재 여부를 과도하게 노출하지 않는다.

## Notes
- MFA는 범위 밖
Behavior update: Updated auth handler and tests.
"
    );
}

#[test]
fn patch_golden_conflict_reports_exact_message_for_repo_fixture() {
    let temp = TestDir::new("patch-golden-conflict");
    let repo_root = temp.path();
    let mut config = test_config(repo_root);
    config.verify.full_workspace_after_accept = false;
    let triad = LocalTriad::new(config);

    write_supporting_runtime_files(repo_root);
    triad
        .init_scaffold(false)
        .expect("scaffold should initialize state");
    copy_repo_claim_fixture(repo_root, "REQ-auth-001.md");
    fs::create_dir_all(repo_root.join("src")).expect("src dir should exist");
    fs::write(repo_root.join("src/auth.rs"), "pub fn login() {}\n")
        .expect("source file should be written");

    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt",
            &BTreeMap::new(),
        )
        .expect("run record should persist");
    triad
        .append_evidence(&test_evidence_with_coverage(
            &triad,
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "auth::login_success",
            &["src/auth.rs"],
        ))
        .expect("pass evidence should append");

    let proposal = triad
        .propose_patch(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("patch proposal should succeed");
    fs::write(
        repo_root.join("spec/claims/REQ-auth-001.md"),
        "\
# REQ-auth-001 Login success

## Claim
사용자는 이미 로그인되어 있어야 한다.

## Examples
- valid credentials -> 200 + session cookie
- wrong password -> 401
- deleted user -> 404

## Invariants
- 비밀번호 원문은 로그에 남지 않는다.
- 실패 응답은 계정 존재 여부를 과도하게 노출하지 않는다.

## Notes
- MFA는 범위 밖
",
    )
    .expect("claim file should be overwritten");

    let error = triad
        .apply_patch(&proposal.patch_id)
        .expect_err("changed claim should conflict");

    assert_eq!(
        error.to_string(),
        "patch conflict: PATCH-000001: claim file no longer matches spec/claims/REQ-auth-001.md"
    );
}

#[test]
fn patch_store_rejects_meta_with_mismatched_diff_path() {
    let temp = TestDir::new("patch-store-mismatch");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let patch_dir = repo_root.join(".triad/patches");

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    fs::write(
        patch_dir.join("PATCH-000001.json"),
        "\
{
  \"id\": \"PATCH-000001\",
  \"claim_id\": \"REQ-auth-001\",
  \"based_on_evidence\": [\"EVID-000001\"],
  \"rationale\": \"Behavior diverged from current spec.\",
  \"created_at\": \"2026-03-10T10:05:00+09:00\",
  \"state\": \"pending\",
  \"diff_path\": \".triad/patches/PATCH-999999.diff\"
}
",
    )
    .expect("patch meta should be written");
    fs::write(
        patch_dir.join("PATCH-000001.diff"),
        "--- a/spec\n+++ b/spec\n@@\n-old\n+new\n",
    )
    .expect("patch diff should be written");

    let error = triad
        .read_patch_draft(&PatchId::new("PATCH-000001").expect("patch id should parse"))
        .expect_err("mismatched diff path should fail");

    assert_eq!(
        error.to_string(),
        "invalid state: patch meta diff path does not match patch id PATCH-000001: .triad/patches/PATCH-999999.diff"
    );
}

#[test]
fn pending_patch_detection_returns_latest_pending_patch_for_claim() {
    let temp = TestDir::new("pending-patch-latest");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000001",
            "REQ-auth-001",
            PatchState::Pending,
        ))
        .expect("first pending patch should store");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000003",
            "REQ-auth-001",
            PatchState::Pending,
        ))
        .expect("latest pending patch should store");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000002",
            "REQ-auth-002",
            PatchState::Pending,
        ))
        .expect("other claim patch should store");

    let pending = triad
        .pending_patch_id_for_claim(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("pending patch detection should succeed");

    assert_eq!(
        pending.expect("pending patch should exist").as_str(),
        "PATCH-000003"
    );
}

#[test]
fn pending_patch_detection_returns_latest_pending_patch_globally() {
    let temp = TestDir::new("pending-patch-global-latest");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000001",
            "REQ-auth-001",
            PatchState::Pending,
        ))
        .expect("first pending patch should store");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000004",
            "REQ-auth-002",
            PatchState::Pending,
        ))
        .expect("global latest pending patch should store");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000003",
            "REQ-auth-003",
            PatchState::Applied,
        ))
        .expect("applied patch should store");

    let pending = triad
        .latest_pending_patch_id()
        .expect("global pending patch detection should succeed");

    assert_eq!(
        pending.expect("pending patch should exist").as_str(),
        "PATCH-000004"
    );
}

#[test]
fn pending_patch_detection_ignores_applied_and_superseded_patches() {
    let temp = TestDir::new("pending-patch-filter");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000001",
            "REQ-auth-001",
            PatchState::Applied,
        ))
        .expect("applied patch should store");
    triad
        .store_patch_draft(&test_patch_draft_with_state(
            "PATCH-000002",
            "REQ-auth-001",
            PatchState::Superseded,
        ))
        .expect("superseded patch should store");

    let pending = triad
        .pending_patch_id_for_claim(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect("pending patch detection should succeed");

    assert!(pending.is_none());
}

#[test]
fn pending_patch_detection_rejects_meta_id_mismatched_with_file_name() {
    let temp = TestDir::new("pending-patch-mismatch");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let patch_dir = repo_root.join(".triad/patches");

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    fs::write(
        patch_dir.join("PATCH-000001.json"),
        "\
{
  \"id\": \"PATCH-000999\",
  \"claim_id\": \"REQ-auth-001\",
  \"based_on_evidence\": [\"EVID-000001\"],
  \"rationale\": \"Behavior diverged from current spec.\",
  \"created_at\": \"2026-03-10T10:05:00+09:00\",
  \"state\": \"pending\",
  \"diff_path\": \".triad/patches/PATCH-000999.diff\"
}
",
    )
    .expect("patch meta should be written");

    let error = triad
        .pending_patch_id_for_claim(&ClaimId::new("REQ-auth-001").expect("claim id should parse"))
        .expect_err("mismatched patch meta id should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "invalid state: patch meta id does not match file name: PATCH-000999 != PATCH-000001 in {}",
            patch_dir.join("PATCH-000001.json").display()
        )
    );
}

#[test]
fn malformed_state_status_rejects_corrupt_evidence_ndjson() {
    let temp = TestDir::new("malformed-state-evidence");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create state");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::write(
        triad.config.paths.evidence_file.as_std_path(),
        "{\"id\":\"EVID-000001\"\n",
    )
    .expect("corrupt evidence row should be written");

    let error = triad
        .status(None)
        .expect_err("corrupt evidence should block status");

    assert_eq!(
        error.to_string(),
        format!(
            "serialization error: invalid evidence row at line 1 in {}: EOF while parsing an object",
            triad.config.paths.evidence_file
        )
    );
}

#[test]
fn malformed_state_status_rejects_corrupt_patch_meta_json() {
    let temp = TestDir::new("malformed-state-patch-meta");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let patch_path = repo_root.join(".triad/patches/PATCH-000001.json");

    triad
        .init_scaffold(false)
        .expect("scaffold should create state");
    write_claim_file(repo_root, "REQ-auth-001", "Login");
    fs::write(&patch_path, "{\"id\":\"PATCH-000001\"\n")
        .expect("corrupt patch meta should be written");

    let error = triad
        .status(None)
        .expect_err("corrupt patch meta should block status");

    assert_eq!(
        error.to_string(),
        format!(
            "serialization error: invalid patch meta {}: EOF while parsing an object",
            patch_path.display()
        )
    );
}

#[test]
fn malformed_state_read_patch_draft_rejects_missing_diff_file() {
    let temp = TestDir::new("malformed-state-missing-diff");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let patch_dir = repo_root.join(".triad/patches");

    triad
        .init_scaffold(false)
        .expect("scaffold should create patch dir");
    fs::write(
        patch_dir.join("PATCH-000001.json"),
        "\
{
  \"id\": \"PATCH-000001\",
  \"claim_id\": \"REQ-auth-001\",
  \"based_on_evidence\": [\"EVID-000001\"],
  \"rationale\": \"Behavior diverged from current spec.\",
  \"created_at\": \"2026-03-10T10:05:00+09:00\",
  \"state\": \"pending\",
  \"diff_path\": \".triad/patches/PATCH-000001.diff\"
}
",
    )
    .expect("patch meta should be written");

    let error = triad
        .read_patch_draft(&PatchId::new("PATCH-000001").expect("patch id should parse"))
        .expect_err("missing patch diff should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "invalid state: patch diff file is missing for PATCH-000001: {}",
            patch_dir.join("PATCH-000001.diff").display()
        )
    );
}

#[test]
fn run_record_store_writes_report_fields_and_metadata() {
    let temp = TestDir::new("run-record-store");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let mut runtime_metadata = BTreeMap::new();
    runtime_metadata.insert("model".to_string(), "gpt-5-codex".to_string());
    runtime_metadata.insert("effort".to_string(), "medium".to_string());

    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    triad
        .store_run_record(
            &test_run_report("RUN-000001"),
            "sha256:prompt-fingerprint",
            &runtime_metadata,
        )
        .expect("run record should store");

    let path = repo_root.join(".triad/runs/RUN-000001.json");
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("run file should be readable"))
            .expect("run json should parse");

    assert_eq!(json["run_id"], "RUN-000001");
    assert_eq!(json["claim_id"], "REQ-auth-001");
    assert_eq!(json["changed_paths"], serde_json::json!(["src/auth.rs"]));
    assert_eq!(
        json["suggested_test_selectors"],
        serde_json::json!(["auth::login_success"])
    );
    assert_eq!(json["blocked_actions"], serde_json::json!(["spec write"]));
    assert_eq!(json["prompt_fingerprint"], "sha256:prompt-fingerprint");
    assert_eq!(json["runtime_metadata"]["model"], "gpt-5-codex");
}

#[test]
fn run_record_store_roundtrips_json_file() {
    let temp = TestDir::new("run-record-roundtrip");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let mut runtime_metadata = BTreeMap::new();
    runtime_metadata.insert("model".to_string(), "gpt-5-codex".to_string());

    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    let report = test_run_report("RUN-000001");
    triad
        .store_run_record(&report, "sha256:prompt-fingerprint", &runtime_metadata)
        .expect("run record should store");

    let record = triad
        .read_run_record(&RunId::new("RUN-000001").expect("run id should parse"))
        .expect("run record should read");

    assert_eq!(record.run_id, report.run_id);
    assert_eq!(record.claim_id, report.claim_id);
    assert_eq!(record.summary, report.summary);
    assert_eq!(record.changed_paths, report.changed_paths);
    assert_eq!(
        record.suggested_test_selectors,
        report.suggested_test_selectors
    );
    assert_eq!(record.blocked_actions, report.blocked_actions);
    assert_eq!(record.needs_patch, report.needs_patch);
    assert_eq!(record.prompt_fingerprint, "sha256:prompt-fingerprint");
    assert_eq!(record.runtime_metadata["model"], "gpt-5-codex");
}

#[test]
fn run_record_store_rejects_non_monotonic_id() {
    let temp = TestDir::new("run-record-monotonic");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create run dir");
    triad
        .store_run_record(&test_run_report("RUN-000001"), "fp-1", &BTreeMap::new())
        .expect("first run record should store");

    let error = triad
        .store_run_record(&test_run_report("RUN-000001"), "fp-2", &BTreeMap::new())
        .expect_err("duplicate run id should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "invalid state: run id must be next monotonic id for {}: expected RUN-000002, got RUN-000001",
            triad.config.paths.run_dir
        )
    );
}

#[test]
fn state_store_roundtrips_evidence_patch_and_run_without_loss() {
    let temp = TestDir::new("state-store-roundtrip");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));
    let evidence = test_evidence("EVID-000001", "REQ-auth-001", "auth::login_success");
    let patch = test_patch_draft("PATCH-000001");
    let run = test_run_report("RUN-000001");
    let mut runtime_metadata = BTreeMap::new();
    runtime_metadata.insert("model".to_string(), "gpt-5-codex".to_string());
    runtime_metadata.insert("effort".to_string(), "medium".to_string());

    triad
        .init_scaffold(false)
        .expect("scaffold should create state dirs");
    triad
        .append_evidence(&evidence)
        .expect("evidence should append");
    triad
        .store_patch_draft(&patch)
        .expect("patch draft should store");
    triad
        .store_run_record(&run, "sha256:prompt-fingerprint", &runtime_metadata)
        .expect("run record should store");

    let evidence_rows = triad.read_evidence().expect("evidence should read");
    let patch_roundtrip = triad
        .read_patch_draft(&patch.id)
        .expect("patch draft should read");
    let run_roundtrip = triad
        .read_run_record(&run.run_id)
        .expect("run record should read");

    assert_eq!(evidence_rows.len(), 1);
    assert_eq!(evidence_rows[0].id, evidence.id);
    assert_eq!(evidence_rows[0].claim_id, evidence.claim_id);
    assert_eq!(evidence_rows[0].command, evidence.command);
    assert_eq!(evidence_rows[0].covered_paths, evidence.covered_paths);
    assert_eq!(evidence_rows[0].covered_digests, evidence.covered_digests);

    assert_eq!(patch_roundtrip.id, patch.id);
    assert_eq!(patch_roundtrip.claim_id, patch.claim_id);
    assert_eq!(patch_roundtrip.based_on_evidence, patch.based_on_evidence);
    assert_eq!(patch_roundtrip.rationale, patch.rationale);
    assert_eq!(patch_roundtrip.created_at, patch.created_at);
    assert_eq!(patch_roundtrip.state, patch.state);
    assert_eq!(patch_roundtrip.unified_diff, patch.unified_diff);

    assert_eq!(run_roundtrip.run_id, run.run_id);
    assert_eq!(run_roundtrip.claim_id, run.claim_id);
    assert_eq!(run_roundtrip.summary, run.summary);
    assert_eq!(run_roundtrip.changed_paths, run.changed_paths);
    assert_eq!(
        run_roundtrip.suggested_test_selectors,
        run.suggested_test_selectors
    );
    assert_eq!(run_roundtrip.blocked_actions, run.blocked_actions);
    assert_eq!(run_roundtrip.needs_patch, run.needs_patch);
    assert_eq!(
        run_roundtrip.prompt_fingerprint,
        "sha256:prompt-fingerprint"
    );
    assert_eq!(run_roundtrip.runtime_metadata, runtime_metadata);
}

#[test]
fn state_store_sequences_do_not_cross_talk_between_artifact_types() {
    let temp = TestDir::new("state-store-sequences");
    let repo_root = temp.path();
    let triad = LocalTriad::new(test_config(repo_root));

    triad
        .init_scaffold(false)
        .expect("scaffold should create state dirs");
    triad
        .append_evidence(&test_evidence("EVID-000001", "REQ-auth-001", "auth::one"))
        .expect("evidence should append");
    triad
        .store_patch_draft(&test_patch_draft("PATCH-000001"))
        .expect("patch draft should store");
    triad
        .store_run_record(&test_run_report("RUN-000001"), "fp-1", &BTreeMap::new())
        .expect("run record should store");

    assert_eq!(
        triad
            .next_evidence_id()
            .expect("next evidence id should read from evidence log")
            .as_str(),
        "EVID-000002"
    );
    assert_eq!(
        triad
            .next_run_id()
            .expect("next run id should read from run dir")
            .as_str(),
        "RUN-000002"
    );
    assert!(
        repo_root.join(".triad/patches/PATCH-000001.json").is_file(),
        "patch store should remain intact"
    );
    assert!(
        repo_root.join(".triad/patches/PATCH-000001.diff").is_file(),
        "patch diff should remain intact"
    );
}

fn test_config(repo_root: &Path) -> CanonicalTriadConfig {
    CanonicalTriadConfig {
        repo_root: utf8(repo_root.to_path_buf()),
        version: 1,
        paths: CanonicalPathConfig {
            claim_dir: utf8(repo_root.join("spec/claims")),
            docs_dir: utf8(repo_root.join("docs")),
            state_dir: utf8(repo_root.join(".triad")),
            evidence_file: utf8(repo_root.join(".triad/evidence.ndjson")),
            patch_dir: utf8(repo_root.join(".triad/patches")),
            run_dir: utf8(repo_root.join(".triad/runs")),
            schema_dir: utf8(repo_root.join("schemas")),
        },
        agent: AgentConfig {
            backend: AgentBackend::Codex,
            model: "gpt-5-codex".into(),
            effort: "medium".into(),
            approval_policy: "never".into(),
            sandbox_policy: "workspace-write".into(),
            timeout_seconds: 600,
            codex: Some(CodexBackendConfig::default()),
            claude: None,
            gemini: None,
        },
        verify: VerifyConfig {
            default_layers: vec!["unit".into(), "contract".into(), "integration".into()],
            full_workspace_after_accept: true,
        },
        guardrails: GuardrailConfig {
            forbid_direct_spec_edits: true,
            forbid_git_commit: true,
            forbid_git_push: true,
            forbid_destructive_rm: true,
        },
    }
}

fn utf8(path: PathBuf) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(path).expect("test path should be valid UTF-8")
}

fn write_supporting_runtime_files(repo_root: &Path) {
    fs::write(
        repo_root.join("AGENTS.md"),
        "# AGENTS.md\n\n- Work on exactly one claim per run.\n",
    )
    .expect("AGENTS.md should be written");
    fs::create_dir_all(repo_root.join("schemas")).expect("schemas dir should exist");
    fs::create_dir_all(repo_root.join("docs")).expect("docs dir should exist");
    fs::write(
        repo_root.join("schemas/agent.run.schema.json"),
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://triad.local/schemas/agent.run.schema.json",
  "title": "Triad Agent Run Response",
  "allOf": [
{
  "$ref": "./envelope.schema.json"
},
{
  "type": "object",
  "properties": {
    "command": {"const": "run"},
    "data": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "claim_id",
        "summary",
        "changed_paths",
        "suggested_test_selectors",
        "blocked_actions",
        "needs_patch"
      ],
      "properties": {
        "claim_id": {
          "type": "string",
          "pattern": "^REQ-[a-z0-9-]+\\-\\d{3}$"
        },
        "summary": {
          "type": "string"
        },
        "changed_paths": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "suggested_test_selectors": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "blocked_actions": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "needs_patch": {
          "type": "boolean"
        },
        "run_id": {
          "type": "string",
          "pattern": "^RUN-\\d{6}$"
        }
      }
    }
  }
}
  ]
}"#,
    )
    .expect("run schema should be written");
    fs::write(
        repo_root.join("schemas/envelope.schema.json"),
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://triad.local/schemas/envelope.schema.json",
  "title": "Triad Agent Envelope",
  "type": "object",
  "additionalProperties": false,
  "required": ["schema_version", "ok", "command", "data", "diagnostics"],
  "properties": {
"schema_version": {"type": "integer", "const": 1},
"ok": {"type": "boolean"},
"command": {"type": "string"},
"data": {"type": "object"},
"diagnostics": {"type": "array"}
  }
}"#,
    )
    .expect("envelope schema should be written");
}

fn valid_claim_body(id: &str, title: &str) -> String {
    format!(
        "\
# {id} {title}

## Claim
User can log in with valid credentials.

## Examples
- valid credentials -> 200 + session cookie

## Invariants
- password plaintext never appears in logs

## Notes
- MFA is out of scope
"
    )
}

fn repo_claim_path(file_name: &str) -> Utf8PathBuf {
    utf8(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../spec/claims")
            .join(file_name),
    )
}

fn copy_repo_claim_fixture(repo_root: &Path, file_name: &str) {
    let claim_path = repo_root.join("spec/claims").join(file_name);
    fs::create_dir_all(claim_path.parent().expect("claim dir should exist"))
        .expect("claim dir should be created");
    fs::write(
        &claim_path,
        fs::read_to_string(repo_claim_path(file_name).as_std_path())
            .expect("repo fixture should be readable"),
    )
    .expect("claim fixture should be copied");
}

fn write_claim_file(repo_root: &Path, id: &str, title: &str) {
    let claim_path = repo_root.join("spec/claims").join(format!("{id}.md"));
    fs::create_dir_all(claim_path.parent().expect("claim dir should exist"))
        .expect("claim dir should be created");
    fs::write(&claim_path, valid_claim_body(id, title)).expect("claim file should be written");
}

fn test_evidence(id: &str, claim_id: &str, selector: &str) -> Evidence {
    test_evidence_with_coverage_and_verdict(
        id,
        claim_id,
        Verdict::Pass,
        selector,
        vec![Utf8PathBuf::from("src/auth.rs")],
        BTreeMap::from([(
            Utf8PathBuf::from("src/auth.rs"),
            "sha256:abc123".to_string(),
        )]),
    )
}

fn test_evidence_with_coverage(
    triad: &LocalTriad,
    id: &str,
    claim_id: &str,
    verdict: Verdict,
    selector: &str,
    covered_paths: &[&str],
) -> Evidence {
    let covered_paths: Vec<Utf8PathBuf> = covered_paths
        .iter()
        .map(|path| Utf8PathBuf::from(*path))
        .collect();
    let covered_digests = triad
        .covered_digests(&covered_paths)
        .expect("covered digests should compute");

    test_evidence_with_coverage_and_verdict(
        id,
        claim_id,
        verdict,
        selector,
        covered_paths,
        covered_digests,
    )
}

fn test_evidence_with_coverage_and_verdict(
    id: &str,
    claim_id: &str,
    verdict: Verdict,
    selector: &str,
    covered_paths: Vec<Utf8PathBuf>,
    covered_digests: BTreeMap<Utf8PathBuf, String>,
) -> Evidence {
    Evidence {
        id: EvidenceId::new(id).expect("evidence id should parse"),
        claim_id: ClaimId::new(claim_id).expect("claim id should parse"),
        kind: EvidenceKind::Integration,
        verdict,
        test_selector: Some(selector.to_string()),
        command: format!("cargo test {selector}"),
        covered_paths,
        covered_digests,
        spec_revision: 1,
        created_at: "2026-03-10T10:00:00+09:00".to_string(),
    }
}

fn test_patch_draft(id: &str) -> PatchDraft {
    test_patch_draft_with_state(id, "REQ-auth-001", PatchState::Pending)
}

fn test_patch_draft_with_state(id: &str, claim_id: &str, state: PatchState) -> PatchDraft {
    PatchDraft {
        id: PatchId::new(id).expect("patch id should parse"),
        claim_id: ClaimId::new(claim_id).expect("claim id should parse"),
        based_on_evidence: vec![
            EvidenceId::new("EVID-000001").expect("evidence id should parse"),
            EvidenceId::new("EVID-000002").expect("evidence id should parse"),
        ],
        unified_diff: "--- a/spec/claims/REQ-auth-001.md\n+++ b/spec/claims/REQ-auth-001.md\n@@\n-old line\n+new line\n".to_string(),
        rationale: "Behavior diverged from current spec.".to_string(),
        created_at: "2026-03-10T10:05:00+09:00".to_string(),
        state,
    }
}

fn test_run_report(id: &str) -> RunClaimReport {
    RunClaimReport {
        run_id: RunId::new(id).expect("run id should parse"),
        claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        summary: "Updated auth handler and tests.".to_string(),
        changed_paths: vec!["src/auth.rs".to_string()],
        suggested_test_selectors: vec!["auth::login_success".to_string()],
        blocked_actions: vec!["spec write".to_string()],
        needs_patch: true,
    }
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let path =
            env::temp_dir().join(format!("triad-runtime-{label}-{}-{unique}", process::id()));

        fs::create_dir_all(&path).expect("temp dir should be created");

        Self { path }
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
