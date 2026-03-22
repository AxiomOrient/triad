use std::collections::BTreeMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use camino::{Utf8Path, Utf8PathBuf};
use triad_core::{
    Claim, ClaimId, Evidence, EvidenceClass, EvidenceId, EvidenceKind, Provenance, TriadError,
    Verdict,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommandCapture;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandCapturePlan {
    repo_root: Utf8PathBuf,
    evidence_id: EvidenceId,
    claim_id: ClaimId,
    claim_revision_digest: String,
    command: String,
    locator: Option<String>,
    artifact_digests: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommandExecution {
    exit_code: i32,
    stdout_len: usize,
    stderr_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandCaptureMetadata {
    commit: Option<String>,
    created_at: String,
}

impl CommandCapture {
    pub fn capture(
        repo_root: &Utf8Path,
        claim: &Claim,
        evidence_id: EvidenceId,
        command: &str,
        locator: Option<&str>,
        artifact_digests: BTreeMap<String, String>,
    ) -> Result<Evidence, TriadError> {
        let plan = Self::plan(
            repo_root,
            claim,
            evidence_id,
            command,
            locator,
            artifact_digests,
        );
        let execution = execute_capture_plan(&plan)?;
        let metadata = capture_metadata(&plan.repo_root)?;
        Ok(build_evidence(plan, execution, metadata))
    }

    fn plan(
        repo_root: &Utf8Path,
        claim: &Claim,
        evidence_id: EvidenceId,
        command: &str,
        locator: Option<&str>,
        artifact_digests: BTreeMap<String, String>,
    ) -> CommandCapturePlan {
        CommandCapturePlan {
            repo_root: repo_root.to_owned(),
            evidence_id,
            claim_id: claim.id.clone(),
            claim_revision_digest: claim.revision_digest.clone(),
            command: command.to_owned(),
            locator: locator.map(ToOwned::to_owned),
            artifact_digests,
        }
    }
}

fn execute_capture_plan(plan: &CommandCapturePlan) -> Result<CommandExecution, TriadError> {
    let output = Command::new("sh")
        .arg("-lc")
        .arg(&plan.command)
        .current_dir(&plan.repo_root)
        .output()
        .map_err(|err| {
            TriadError::Io(format!(
                "failed to execute verify command `{}`: {err}",
                plan.command
            ))
        })?;

    let exit_code = output.status.code().ok_or_else(|| {
        TriadError::InvalidState(format!(
            "verify command terminated without exit code: {}",
            plan.command
        ))
    })?;

    Ok(CommandExecution {
        exit_code,
        stdout_len: output.stdout.len(),
        stderr_len: output.stderr.len(),
    })
}

fn capture_metadata(repo_root: &Utf8Path) -> Result<CommandCaptureMetadata, TriadError> {
    Ok(CommandCaptureMetadata {
        commit: git_commit(repo_root),
        created_at: created_at_now()?,
    })
}

fn build_evidence(
    plan: CommandCapturePlan,
    execution: CommandExecution,
    metadata: CommandCaptureMetadata,
) -> Evidence {
    Evidence {
        id: plan.evidence_id,
        claim_id: plan.claim_id,
        class: EvidenceClass::Hard,
        kind: EvidenceKind::Test,
        verdict: verdict_from_exit_code(execution.exit_code),
        verifier: "shell".into(),
        claim_revision_digest: plan.claim_revision_digest,
        artifact_digests: plan.artifact_digests,
        command: Some(plan.command),
        locator: plan.locator,
        summary: Some(command_summary(execution)),
        provenance: Provenance {
            actor: "system".into(),
            runtime: Some("shell".into()),
            session_id: None,
            task_id: None,
            workflow_id: None,
            commit: metadata.commit,
            environment_digest: None,
        },
        created_at: metadata.created_at,
    }
}

fn verdict_from_exit_code(exit_code: i32) -> Verdict {
    if exit_code == 0 {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

fn command_summary(execution: CommandExecution) -> String {
    format!(
        "command exited with status {exit_code} (stdout: {} bytes, stderr: {} bytes)",
        execution.stdout_len,
        execution.stderr_len,
        exit_code = execution.exit_code
    )
}

fn git_commit(repo_root: &Utf8Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root.as_str())
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn created_at_now() -> Result<String, TriadError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| {
            TriadError::InvalidState(format!("system clock is before unix epoch: {err}"))
        })?
        .as_secs();
    Ok(format!("unix:{seconds}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::{Utf8Path, Utf8PathBuf};
    use triad_core::{Claim, ClaimId, EvidenceId, Verdict};

    use super::{CommandCapture, CommandCaptureMetadata, CommandExecution, build_evidence};

    fn temp_dir(label: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "triad-fs-command-capture-{label}-{}-{unique}",
            process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should create");
        Utf8PathBuf::from_path_buf(path).expect("utf8 temp path")
    }

    fn claim() -> Claim {
        Claim {
            id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            title: "Login success".into(),
            statement: "System grants access with valid credentials.".into(),
            examples: vec!["valid -> 200".into()],
            invariants: vec!["session issued".into()],
            notes: None,
            revision_digest: "sha256:claim".into(),
        }
    }

    #[test]
    fn capture_maps_success_and_failure_to_evidence() {
        let success = CommandCapture::capture(
            &temp_dir("success"),
            &claim(),
            EvidenceId::from_sequence(1).expect("evidence id should format"),
            "true",
            None,
            BTreeMap::new(),
        )
        .expect("true should execute");
        let failure = CommandCapture::capture(
            &temp_dir("failure"),
            &claim(),
            EvidenceId::from_sequence(2).expect("evidence id should format"),
            "false",
            None,
            BTreeMap::new(),
        )
        .expect("false should execute");

        assert_eq!(success.verdict, Verdict::Pass);
        assert_eq!(failure.verdict, Verdict::Fail);
    }

    #[test]
    fn build_evidence_uses_planned_inputs_without_io() {
        let plan = CommandCapture::plan(
            Utf8Path::new("/repo"),
            &claim(),
            EvidenceId::from_sequence(5).expect("evidence id should format"),
            "cargo test auth::login_success",
            Some("cargo-test:REQ-auth-001"),
            BTreeMap::from([("src/auth.rs".into(), "sha256:file".into())]),
        );
        let execution = CommandExecution {
            exit_code: 0,
            stdout_len: 12,
            stderr_len: 4,
        };
        let metadata = CommandCaptureMetadata {
            commit: Some("abc123".into()),
            created_at: "unix:42".into(),
        };

        let evidence = build_evidence(plan, execution, metadata);

        assert_eq!(evidence.claim_id.as_str(), "REQ-auth-001");
        assert_eq!(
            evidence.command.as_deref(),
            Some("cargo test auth::login_success")
        );
        assert_eq!(evidence.locator.as_deref(), Some("cargo-test:REQ-auth-001"));
        assert_eq!(evidence.provenance.commit.as_deref(), Some("abc123"));
        assert_eq!(evidence.created_at, "unix:42");
        assert_eq!(
            evidence.summary.as_deref(),
            Some("command exited with status 0 (stdout: 12 bytes, stderr: 4 bytes)")
        );
    }

    #[test]
    fn capture_summarizes_command_output_without_printing_contract_data() {
        let evidence = CommandCapture::capture(
            &temp_dir("summary"),
            &claim(),
            EvidenceId::from_sequence(3).expect("evidence id should format"),
            "printf 'stdout-bytes'; printf 'stderr-bytes' >&2",
            None,
            BTreeMap::new(),
        )
        .expect("printf should execute");

        assert_eq!(evidence.verdict, Verdict::Pass);
        assert_eq!(
            evidence.summary.as_deref(),
            Some("command exited with status 0 (stdout: 12 bytes, stderr: 12 bytes)")
        );
    }

    #[test]
    fn capture_populates_locator_and_runtime_metadata() {
        let evidence = CommandCapture::capture(
            &temp_dir("metadata"),
            &claim(),
            EvidenceId::from_sequence(4).expect("evidence id should format"),
            "true",
            Some("cargo-test:REQ-auth-001"),
            BTreeMap::new(),
        )
        .expect("true should execute");

        assert_eq!(evidence.locator.as_deref(), Some("cargo-test:REQ-auth-001"));
        assert_eq!(evidence.provenance.runtime.as_deref(), Some("shell"));
        assert!(evidence.created_at.starts_with("unix:"));
    }
}
