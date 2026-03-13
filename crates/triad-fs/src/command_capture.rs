use std::collections::BTreeMap;
use std::process::Command;

use triad_core::{
    Claim, Evidence, EvidenceClass, EvidenceId, EvidenceKind, Provenance, TriadError, Verdict,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommandCapture;

impl CommandCapture {
    pub fn capture(
        claim: &Claim,
        evidence_id: EvidenceId,
        command: &str,
        artifact_digests: BTreeMap<String, String>,
    ) -> Result<Evidence, TriadError> {
        let output = Command::new("sh")
            .arg("-lc")
            .arg(command)
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
            locator: None,
            summary: Some(command_summary(exit_code, &output.stdout, &output.stderr)),
            provenance: Provenance {
                actor: "system".into(),
                runtime: Some("shell".into()),
                session_id: None,
                task_id: None,
                workflow_id: None,
                commit: None,
                environment_digest: None,
            },
            created_at: "unix:0".into(),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use triad_core::{Claim, ClaimId, EvidenceId, Verdict};

    use super::CommandCapture;

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
            &claim(),
            EvidenceId::from_sequence(1).expect("evidence id should format"),
            "true",
            BTreeMap::new(),
        )
        .expect("true should execute");
        let failure = CommandCapture::capture(
            &claim(),
            EvidenceId::from_sequence(2).expect("evidence id should format"),
            "false",
            BTreeMap::new(),
        )
        .expect("false should execute");

        assert_eq!(success.verdict, Verdict::Pass);
        assert_eq!(failure.verdict, Verdict::Fail);
    }

    #[test]
    fn capture_summarizes_command_output_without_printing_contract_data() {
        let evidence = CommandCapture::capture(
            &claim(),
            EvidenceId::from_sequence(3).expect("evidence id should format"),
            "printf 'stdout-bytes'; printf 'stderr-bytes' >&2",
            BTreeMap::new(),
        )
        .expect("printf should execute");

        assert_eq!(evidence.verdict, Verdict::Pass);
        assert_eq!(
            evidence.summary.as_deref(),
            Some("command exited with status 0 (stdout: 12 bytes, stderr: 12 bytes)")
        );
    }
}
