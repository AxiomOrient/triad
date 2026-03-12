use std::collections::BTreeMap;

use crate::{Claim, Evidence};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceFreshness {
    Fresh,
    StaleClaimRevision,
    StaleArtifacts,
    StaleBoth,
}

pub fn classify_evidence_freshness(
    claim: &Claim,
    current_artifacts: &BTreeMap<String, String>,
    evidence: &Evidence,
) -> EvidenceFreshness {
    match (
        evidence.claim_revision_digest != claim.revision_digest,
        evidence.artifact_digests != *current_artifacts,
    ) {
        (false, false) => EvidenceFreshness::Fresh,
        (true, false) => EvidenceFreshness::StaleClaimRevision,
        (false, true) => EvidenceFreshness::StaleArtifacts,
        (true, true) => EvidenceFreshness::StaleBoth,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        Claim, ClaimId, Evidence, EvidenceClass, EvidenceId, EvidenceKind, Provenance, Verdict,
    };

    use super::{EvidenceFreshness, classify_evidence_freshness};

    fn sample_claim(revision_digest: &str) -> Claim {
        Claim {
            id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            title: "Login success".into(),
            statement: "System grants access with valid credentials.".into(),
            examples: vec!["valid -> 200".into()],
            invariants: vec!["session issued".into()],
            notes: None,
            revision_digest: revision_digest.into(),
        }
    }

    fn sample_evidence(
        revision_digest: &str,
        artifact_digests: BTreeMap<String, String>,
    ) -> Evidence {
        Evidence {
            id: EvidenceId::new("EVID-000001").expect("evidence id should parse"),
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            class: EvidenceClass::Hard,
            kind: EvidenceKind::Test,
            verdict: Verdict::Pass,
            verifier: "cargo test".into(),
            claim_revision_digest: revision_digest.into(),
            artifact_digests,
            command: Some("cargo test".into()),
            locator: None,
            summary: None,
            provenance: Provenance {
                actor: "ci".into(),
                runtime: Some("cargo-test".into()),
                session_id: None,
                task_id: None,
                workflow_id: None,
                commit: None,
                environment_digest: None,
            },
            created_at: "2026-03-12T00:00:00Z".into(),
        }
    }

    fn current_artifacts() -> BTreeMap<String, String> {
        BTreeMap::from([("src/auth.rs".into(), "sha256:file".into())])
    }

    #[test]
    fn freshness_classifies_fresh_evidence() {
        let claim = sample_claim("sha256:claim");
        let evidence = sample_evidence("sha256:claim", current_artifacts());

        assert_eq!(
            classify_evidence_freshness(&claim, &current_artifacts(), &evidence),
            EvidenceFreshness::Fresh
        );
    }

    #[test]
    fn freshness_classifies_stale_claim_revision() {
        let claim = sample_claim("sha256:new-claim");
        let evidence = sample_evidence("sha256:old-claim", current_artifacts());

        assert_eq!(
            classify_evidence_freshness(&claim, &current_artifacts(), &evidence),
            EvidenceFreshness::StaleClaimRevision
        );
    }

    #[test]
    fn freshness_classifies_stale_artifacts() {
        let claim = sample_claim("sha256:claim");
        let evidence = sample_evidence(
            "sha256:claim",
            BTreeMap::from([("src/auth.rs".into(), "sha256:old-file".into())]),
        );

        assert_eq!(
            classify_evidence_freshness(&claim, &current_artifacts(), &evidence),
            EvidenceFreshness::StaleArtifacts
        );
    }

    #[test]
    fn freshness_classifies_stale_both() {
        let claim = sample_claim("sha256:new-claim");
        let evidence = sample_evidence(
            "sha256:old-claim",
            BTreeMap::from([("src/auth.rs".into(), "sha256:old-file".into())]),
        );

        assert_eq!(
            classify_evidence_freshness(&claim, &current_artifacts(), &evidence),
            EvidenceFreshness::StaleBoth
        );
    }
}
