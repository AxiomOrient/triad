use crate::{ClaimStatus, Evidence, Verdict};

pub fn short_revision(revision_digest: &str) -> &str {
    let digest = revision_digest
        .strip_prefix("sha256:")
        .unwrap_or(revision_digest);
    &digest[..digest.len().min(12)]
}

pub fn status_reason(status: ClaimStatus) -> &'static str {
    match status {
        ClaimStatus::Confirmed => "fresh hard pass exists",
        ClaimStatus::Contradicted => "fresh hard fail exists",
        ClaimStatus::Blocked => "fresh hard unknown exists",
        ClaimStatus::Stale => "only stale hard evidence exists",
        ClaimStatus::Unsupported => "no hard evidence exists",
    }
}

pub fn strongest_verdict_for(evidence: &[&Evidence]) -> Option<Verdict> {
    if evidence.iter().any(|item| item.verdict == Verdict::Fail) {
        Some(Verdict::Fail)
    } else if evidence.iter().any(|item| item.verdict == Verdict::Pass) {
        Some(Verdict::Pass)
    } else if evidence.iter().any(|item| item.verdict == Verdict::Unknown) {
        Some(Verdict::Unknown)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{ClaimId, Evidence, EvidenceClass, EvidenceId, EvidenceKind, Provenance, Verdict};

    use super::{short_revision, status_reason, strongest_verdict_for};

    fn evidence(id: &str, verdict: Verdict) -> Evidence {
        Evidence {
            id: EvidenceId::new(id).expect("evidence id should parse"),
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            class: EvidenceClass::Hard,
            kind: EvidenceKind::Test,
            verdict,
            verifier: "cargo test".into(),
            claim_revision_digest: "sha256:claim".into(),
            artifact_digests: BTreeMap::new(),
            command: None,
            locator: None,
            summary: None,
            provenance: Provenance {
                actor: "ci".into(),
                runtime: None,
                session_id: None,
                task_id: None,
                workflow_id: None,
                commit: None,
                environment_digest: None,
            },
            created_at: "2026-03-12T00:00:00Z".into(),
        }
    }

    #[test]
    fn short_revision_uses_digest_prefix_if_present() {
        assert_eq!(short_revision("sha256:1234567890abcdef"), "1234567890ab");
        assert_eq!(short_revision("abcdef"), "abcdef");
    }

    #[test]
    fn strongest_verdict_prefers_fail_then_pass_then_unknown() {
        let pass = evidence("EVID-000001", Verdict::Pass);
        let fail = evidence("EVID-000002", Verdict::Fail);
        let unknown = evidence("EVID-000003", Verdict::Unknown);

        assert_eq!(
            strongest_verdict_for(&[&pass, &unknown]),
            Some(Verdict::Pass)
        );
        assert_eq!(
            strongest_verdict_for(&[&pass, &fail, &unknown]),
            Some(Verdict::Fail)
        );
        assert_eq!(strongest_verdict_for(&[&unknown]), Some(Verdict::Unknown));
        assert_eq!(strongest_verdict_for(&[]), None);
    }

    #[test]
    fn status_reason_maps_each_status() {
        assert_eq!(
            status_reason(crate::ClaimStatus::Confirmed),
            "fresh hard pass exists"
        );
        assert_eq!(
            status_reason(crate::ClaimStatus::Contradicted),
            "fresh hard fail exists"
        );
    }
}
