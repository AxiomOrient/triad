use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use camino::Utf8Path;
use camino::Utf8PathBuf;
use sha2::{Digest, Sha256};
use triad_core::{
    ClaimId, Evidence, EvidenceId, EvidenceKind, TriadApi, TriadError, Verdict, VerifyLayer,
    VerifyReport, VerifyRequest,
};

use crate::repo_support::{
    sha256_prefixed_hex, unique_non_empty_strings, validate_repo_relative_path,
};
use crate::storage::read_evidence;
use crate::{LocalTriad, RunRecord, parsed_claim_by_id_or_issue, read_run_records};

#[derive(Debug, Clone, Default)]
pub struct RelevantEvidenceSet {
    pub pass: Option<Evidence>,
    pub fail: Option<Evidence>,
    pub unknown: Option<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyCommandPlan {
    pub layer: VerifyLayer,
    pub command: String,
    pub targeted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyCommandExecution {
    pub plan: VerifyCommandPlan,
    pub exit_code: i32,
    pub success: bool,
}

pub(crate) trait VerifyCommandRunner {
    fn run(&self, plan: &VerifyCommandPlan) -> Result<i32, TriadError>;
}

pub(crate) struct ProcessCommandRunner;

impl VerifyCommandRunner for ProcessCommandRunner {
    fn run(&self, plan: &VerifyCommandPlan) -> Result<i32, TriadError> {
        let status = Command::new("sh")
            .arg("-lc")
            .arg(&plan.command)
            .status()
            .map_err(|err| {
                TriadError::Io(format!(
                    "failed to execute verify command `{}`: {err}",
                    plan.command
                ))
            })?;

        status.code().ok_or_else(|| {
            TriadError::RuntimeBlocked(format!(
                "verify command terminated without exit code: {}",
                plan.command
            ))
        })
    }
}

pub(crate) fn execute_verify_commands_with_runner<R: VerifyCommandRunner>(
    triad: &LocalTriad,
    req: &VerifyRequest,
    runner: &R,
) -> Result<Vec<VerifyCommandExecution>, TriadError> {
    let plans = triad.plan_verify_commands(req)?;
    let mut executions = Vec::with_capacity(plans.len());

    for plan in plans {
        let exit_code = runner.run(&plan)?;
        executions.push(VerifyCommandExecution {
            success: exit_code == 0,
            plan,
            exit_code,
        });
    }

    Ok(executions)
}

pub(crate) fn verify_layer_kind(layer: VerifyLayer) -> EvidenceKind {
    match layer {
        VerifyLayer::Unit => EvidenceKind::Unit,
        VerifyLayer::Contract => EvidenceKind::Contract,
        VerifyLayer::Integration => EvidenceKind::Integration,
        VerifyLayer::Probe => EvidenceKind::Probe,
    }
}

pub(crate) fn planned_test_selector(
    plan: &VerifyCommandPlan,
) -> Result<Option<String>, TriadError> {
    if !plan.targeted {
        return Ok(None);
    }

    let selector = match plan.layer {
        VerifyLayer::Unit => plan.command.strip_prefix("cargo test --lib "),
        VerifyLayer::Contract => plan.command.strip_prefix("cargo test "),
        VerifyLayer::Integration => plan.command.strip_prefix("cargo test --tests "),
        VerifyLayer::Probe => plan
            .command
            .strip_prefix("cargo test --tests ")
            .and_then(|command| command.strip_suffix(" -- --ignored")),
    }
    .ok_or_else(|| {
        TriadError::InvalidState(format!(
            "unable to extract targeted selector from verify command: {}",
            plan.command
        ))
    })?;

    Ok(Some(selector.to_string()))
}

pub(crate) fn current_timestamp_string() -> Result<String, TriadError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| {
            TriadError::RuntimeBlocked(format!("system clock before unix epoch: {err}"))
        })?
        .as_secs();
    Ok(format!("unix:{seconds}"))
}

pub(crate) fn verify_covered_paths_for_claim(
    triad: &LocalTriad,
    claim_id: &ClaimId,
) -> Result<Vec<Utf8PathBuf>, TriadError> {
    let Some(record) =
        latest_run_record_for_claim(triad.config.paths.run_dir.as_std_path(), claim_id)?
    else {
        return Ok(Vec::new());
    };

    normalized_run_changed_paths(&record)?
        .filter_map(|result| match result {
            Ok(path) if triad.config.repo_root.join(&path).is_file() => Some(Ok(path)),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .collect()
}

pub(crate) fn normalized_run_changed_paths(
    record: &RunRecord,
) -> Result<impl Iterator<Item = Result<Utf8PathBuf, TriadError>>, TriadError> {
    Ok(unique_non_empty_strings(record.changed_paths.clone())
        .into_iter()
        .map(|path| {
            let path = Utf8PathBuf::from(path);
            validate_repo_relative_path(&path, "run changed path")?;
            Ok(path)
        }))
}

pub(crate) fn latest_run_record_for_claim(
    run_dir: &Path,
    claim_id: &ClaimId,
) -> Result<Option<RunRecord>, TriadError> {
    let latest = read_run_records(run_dir)?
        .into_iter()
        .filter(|r| &r.claim_id == claim_id)
        .max_by_key(|r| r.run_id.sequence_number());
    Ok(latest)
}

pub(crate) fn covered_path_digests(
    repo_root: &Path,
    covered_paths: &[Utf8PathBuf],
) -> Result<BTreeMap<Utf8PathBuf, String>, TriadError> {
    let mut digests = BTreeMap::new();

    for covered_path in covered_paths {
        digests.insert(
            covered_path.clone(),
            covered_path_digest(repo_root, covered_path)?,
        );
    }

    Ok(digests)
}

pub(crate) fn evidence_is_fresh(repo_root: &Path, evidence: &Evidence) -> Result<bool, TriadError> {
    let mut current_digests = BTreeMap::new();

    for covered_path in &evidence.covered_paths {
        let path = resolve_covered_path(repo_root, covered_path)?;
        if !path.is_file() {
            return Ok(false);
        }

        current_digests.insert(
            covered_path.clone(),
            covered_path_digest(repo_root, covered_path)?,
        );
    }

    Ok(current_digests == evidence.covered_digests)
}

pub(crate) fn relevant_evidence_for_claim(
    repo_root: &Path,
    evidence_path: &Path,
    claim_id: &ClaimId,
) -> Result<RelevantEvidenceSet, TriadError> {
    let all = read_evidence(evidence_path)?;
    relevant_evidence_from_slice(repo_root, &all, claim_id)
}

pub(crate) fn relevant_evidence_from_slice(
    repo_root: &Path,
    all_evidence: &[Evidence],
    claim_id: &ClaimId,
) -> Result<RelevantEvidenceSet, TriadError> {
    let mut selected = RelevantEvidenceSet::default();

    for evidence in all_evidence {
        if &evidence.claim_id != claim_id || !evidence_is_fresh(repo_root, evidence)? {
            continue;
        }

        let slot = match evidence.verdict {
            Verdict::Pass => &mut selected.pass,
            Verdict::Fail => &mut selected.fail,
            Verdict::Unknown => &mut selected.unknown,
        };
        let replace = slot
            .as_ref()
            .map(|current| evidence.id.sequence_number() > current.id.sequence_number())
            .unwrap_or(true);
        if replace {
            *slot = Some(evidence.clone());
        }
    }

    Ok(selected)
}

pub(crate) fn latest_relevant_evidence(relevant: &RelevantEvidenceSet) -> Option<&Evidence> {
    [
        relevant.pass.as_ref(),
        relevant.fail.as_ref(),
        relevant.unknown.as_ref(),
    ]
    .into_iter()
    .flatten()
    .max_by_key(|evidence| evidence.id.sequence_number())
}

pub(crate) fn verify_claim_with_runner<R: VerifyCommandRunner>(
    triad: &LocalTriad,
    req: VerifyRequest,
    runner: &R,
) -> Result<VerifyReport, TriadError> {
    if req.layers.is_empty() {
        return Err(TriadError::InvalidState(
            "verify request must include at least one layer".to_string(),
        ));
    }

    let claim = parsed_claim_by_id_or_issue(triad, &req.claim_id)?;
    let mut executions = execute_verify_commands_with_runner(triad, &req, runner)?;
    let covered_paths = verify_covered_paths_for_claim(triad, &req.claim_id)?;
    let covered_digests = triad.covered_digests(&covered_paths)?;
    let overall_verdict = if executions.iter().all(|execution| execution.success) {
        Verdict::Pass
    } else {
        Verdict::Fail
    };

    executions.sort_by_key(|execution| !execution.success);

    let first_evidence_id = triad.next_evidence_id()?;
    let mut evidence_ids = Vec::with_capacity(executions.len());
    for (offset, execution) in executions.into_iter().enumerate() {
        let evidence = Evidence {
            id: EvidenceId::from_sequence(first_evidence_id.sequence_number() + offset as u32)?,
            claim_id: req.claim_id.clone(),
            kind: verify_layer_kind(execution.plan.layer),
            verdict: if execution.success {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            test_selector: planned_test_selector(&execution.plan)?,
            command: execution.plan.command,
            covered_paths: covered_paths.clone(),
            covered_digests: covered_digests.clone(),
            spec_revision: claim.revision,
            created_at: current_timestamp_string()?,
        };
        triad.append_evidence(&evidence)?;
        evidence_ids.push(evidence.id);
    }

    let drift = triad.detect_drift(&req.claim_id)?;

    Ok(VerifyReport {
        claim_id: req.claim_id,
        verdict: overall_verdict,
        layers: req.layers,
        full_workspace: req.full_workspace,
        evidence_ids,
        status_after_verify: drift.status,
        pending_patch_id: drift.pending_patch_id,
    })
}

fn covered_path_digest(repo_root: &Path, covered_path: &Utf8Path) -> Result<String, TriadError> {
    let path = resolve_covered_path(repo_root, covered_path)?;
    let mut file = File::open(&path).map_err(|err| {
        TriadError::Io(format!(
            "failed to open covered path {}: {err}",
            path.display()
        ))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file.read(&mut buffer).map_err(|err| {
            TriadError::Io(format!(
                "failed to read covered path {}: {err}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(sha256_prefixed_hex(&hasher.finalize()))
}

fn resolve_covered_path(repo_root: &Path, covered_path: &Utf8Path) -> Result<PathBuf, TriadError> {
    validate_repo_relative_path(covered_path, "covered path")?;
    Ok(repo_root.join(covered_path.as_std_path()))
}
