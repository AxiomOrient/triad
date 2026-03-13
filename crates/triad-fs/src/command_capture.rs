use std::collections::BTreeMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use camino::Utf8Path;
use triad_core::{
    Claim, Evidence, EvidenceClass, EvidenceId, EvidenceKind, Provenance, TriadError, Verdict,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommandCapture;

impl CommandCapture {
    pub fn capture(
        repo_root: &Utf8Path,
        claim: &Claim,
        evidence_id: EvidenceId,
        command: &str,
        locator: Option<&str>,
        artifact_digests: BTreeMap<String, String>,
    ) -> Result<Evidence, TriadError> {
        let output = Command::new("sh")
            .arg("-lc")
            .arg(command)
            .current_dir(repo_root)
            .output()
            .map_err(|err| {
                TriadError::Io(format!(
                    "failed to execute verify command `{command}`: {err}"
                ))
            })?;

        let exit_code = output.status.code().ok_or_else(|| {
            TriadError::InvalidState(format!(
                "verify command terminated without exit code: {command}"
            ))
        })?;

        Ok(Evidence {
            id: evidence_id,
            claim_id: claim.id.clone(),
            class: EvidenceClass::Hard,
            kind: EvidenceKind::Test,
            verdict: if exit_code == 0 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            verifier: "shell".into(),
            claim_revision_digest: claim.revision_digest.clone(),
            artifact_digests,
            command: Some(command.into()),
            locator: locator.map(ToOwned::to_owned),
            summary: Some(command_summary(exit_code, &output.stdout, &output.stderr)),
            provenance: Provenance {
                actor: "system".into(),
                runtime: Some("shell".into()),
                session_id: None,
                task_id: None,
                workflow_id: None,
                commit: git_commit(repo_root),
                environment_digest: None,
            },
            created_at: created_at_now()?,
        })
    }
}

fn command_summary(exit_code: i32, stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "command exited with status {exit_code} (stdout: {} bytes, stderr: {} bytes)",
        stdout.len(),
        stderr.len()
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

    use camino::Utf8PathBuf;
    use triad_core::{Claim, ClaimId, EvidenceId, Verdict};

    use super::CommandCapture;

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
