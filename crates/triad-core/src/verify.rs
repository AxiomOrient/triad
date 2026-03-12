use std::collections::BTreeMap;

use crate::{
    Claim, ClaimId, ClaimReport, ClaimStatus, Evidence, EvidenceClass, EvidenceFreshness,
    EvidenceId, Verdict, classify_evidence_freshness, status_reason, strongest_verdict_for,
};

pub fn verify_claim(
    claim: &Claim,
    current_artifacts: &BTreeMap<String, String>,
    evidence: &[Evidence],
) -> ClaimReport {
    let mut fresh_hard = Vec::<&Evidence>::new();
    let mut stale_hard = Vec::<&Evidence>::new();
    let mut fresh_evidence_ids = Vec::<EvidenceId>::new();
    let mut stale_evidence_ids = Vec::<EvidenceId>::new();
    let mut advisory_evidence_ids = Vec::<EvidenceId>::new();
    let mut reasons = Vec::<String>::new();

    for item in evidence.iter().filter(|item| item.claim_id == claim.id) {
        if item.class == EvidenceClass::Advisory {
            advisory_evidence_ids.push(item.id.clone());
            if let Some(summary) = item.summary.as_deref() {
                reasons.push(summary.to_string());
            }
            continue;
        }

        match classify_evidence_freshness(claim, current_artifacts, item) {
            EvidenceFreshness::Fresh => {
                fresh_evidence_ids.push(item.id.clone());
                fresh_hard.push(item);
            }
            EvidenceFreshness::StaleClaimRevision
            | EvidenceFreshness::StaleArtifacts
            | EvidenceFreshness::StaleBoth => {
                stale_evidence_ids.push(item.id.clone());
                stale_hard.push(item);
            }
        }
    }

    let status = if fresh_hard.iter().any(|item| item.verdict == Verdict::Fail) {
        ClaimStatus::Contradicted
    } else if fresh_hard.iter().any(|item| item.verdict == Verdict::Pass) {
        ClaimStatus::Confirmed
    } else if fresh_hard
        .iter()
        .any(|item| item.verdict == Verdict::Unknown)
    {
        ClaimStatus::Blocked
    } else if !stale_hard.is_empty() {
        ClaimStatus::Stale
    } else {
        ClaimStatus::Unsupported
    };

    reasons.insert(0, status_reason(status).to_string());

    ClaimReport {
        claim_id: claim.id.clone(),
        status,
        reasons,
        fresh_evidence_ids,
        stale_evidence_ids,
        advisory_evidence_ids,
        strongest_verdict: strongest_verdict_for(if !fresh_hard.is_empty() {
            &fresh_hard
        } else {
            &stale_hard
        }),
    }
}

pub fn verify_many(
    claims: &[Claim],
    snapshots: &BTreeMap<ClaimId, BTreeMap<String, String>>,
    evidence: &[Evidence],
) -> Vec<ClaimReport> {
    claims
        .iter()
        .map(|claim| {
            let current_artifacts = snapshots.get(&claim.id).cloned().unwrap_or_default();
            verify_claim(claim, &current_artifacts, evidence)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        Claim, ClaimId, ClaimStatus, Evidence, EvidenceClass, EvidenceId, EvidenceKind, Provenance,
        Verdict,
    };

    use super::{verify_claim, verify_many};

    fn claim(id: &str, digest: &str) -> Claim {
        Claim {
            id: ClaimId::new(id).expect("claim id should parse"),
            title: format!("{id} title"),
            statement: "Statement".into(),
            examples: vec!["example".into()],
            invariants: vec!["invariant".into()],
            notes: None,
            revision_digest: digest.into(),
        }
    }

    fn hard_evidence(
        id: &str,
        claim_id: &str,
        verdict: Verdict,
        revision_digest: &str,
        artifact_digests: BTreeMap<String, String>,
    ) -> Evidence {
        Evidence {
            id: EvidenceId::new(id).expect("evidence id should parse"),
            claim_id: ClaimId::new(claim_id).expect("claim id should parse"),
            class: EvidenceClass::Hard,
            kind: EvidenceKind::Test,
            verdict,
            verifier: "cargo test".into(),
            claim_revision_digest: revision_digest.into(),
            artifact_digests,
            command: Some("cargo test".into()),
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

    fn advisory_evidence(id: &str, claim_id: &str, summary: &str) -> Evidence {
        Evidence {
            id: EvidenceId::new(id).expect("evidence id should parse"),
            claim_id: ClaimId::new(claim_id).expect("claim id should parse"),
            class: EvidenceClass::Advisory,
            kind: EvidenceKind::Analysis,
            verdict: Verdict::Pass,
            verifier: "human".into(),
            claim_revision_digest: "sha256:ignored".into(),
            artifact_digests: BTreeMap::new(),
            command: None,
            locator: None,
            summary: Some(summary.into()),
            provenance: Provenance {
                actor: "human".into(),
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

    fn current_artifacts() -> BTreeMap<String, String> {
        BTreeMap::from([("src/auth.rs".into(), "sha256:file".into())])
    }

    #[test]
    fn verify_claim_marks_confirmed_with_fresh_hard_pass() {
        let claim = claim("REQ-auth-001", "sha256:claim");
        let evidence = vec![hard_evidence(
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "sha256:claim",
            current_artifacts(),
        )];

        let report = verify_claim(&claim, &current_artifacts(), &evidence);

        assert_eq!(report.status, ClaimStatus::Confirmed);
        assert_eq!(report.strongest_verdict, Some(Verdict::Pass));
        assert_eq!(report.fresh_evidence_ids.len(), 1);
    }

    #[test]
    fn verify_claim_prefers_fresh_hard_fail_over_pass() {
        let claim = claim("REQ-auth-001", "sha256:claim");
        let evidence = vec![
            hard_evidence(
                "EVID-000001",
                "REQ-auth-001",
                Verdict::Pass,
                "sha256:claim",
                current_artifacts(),
            ),
            hard_evidence(
                "EVID-000002",
                "REQ-auth-001",
                Verdict::Fail,
                "sha256:claim",
                current_artifacts(),
            ),
        ];

        let report = verify_claim(&claim, &current_artifacts(), &evidence);

        assert_eq!(report.status, ClaimStatus::Contradicted);
        assert_eq!(report.strongest_verdict, Some(Verdict::Fail));
        assert_eq!(report.fresh_evidence_ids.len(), 2);
    }

    #[test]
    fn verify_claim_marks_blocked_with_only_fresh_unknown() {
        let claim = claim("REQ-auth-001", "sha256:claim");
        let evidence = vec![hard_evidence(
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Unknown,
            "sha256:claim",
            current_artifacts(),
        )];

        let report = verify_claim(&claim, &current_artifacts(), &evidence);

        assert_eq!(report.status, ClaimStatus::Blocked);
        assert_eq!(report.strongest_verdict, Some(Verdict::Unknown));
    }

    #[test]
    fn verify_claim_marks_stale_when_only_stale_hard_evidence_exists() {
        let claim = claim("REQ-auth-001", "sha256:new-claim");
        let evidence = vec![hard_evidence(
            "EVID-000001",
            "REQ-auth-001",
            Verdict::Pass,
            "sha256:old-claim",
            current_artifacts(),
        )];

        let report = verify_claim(&claim, &current_artifacts(), &evidence);

        assert_eq!(report.status, ClaimStatus::Stale);
        assert_eq!(report.stale_evidence_ids.len(), 1);
        assert_eq!(report.strongest_verdict, Some(Verdict::Pass));
    }

    #[test]
    fn verify_claim_marks_unsupported_without_hard_evidence() {
        let claim = claim("REQ-auth-001", "sha256:claim");
        let evidence = vec![advisory_evidence(
            "EVID-000001",
            "REQ-auth-001",
            "manual note only",
        )];

        let report = verify_claim(&claim, &current_artifacts(), &evidence);

        assert_eq!(report.status, ClaimStatus::Unsupported);
        assert_eq!(report.advisory_evidence_ids.len(), 1);
        assert_eq!(report.strongest_verdict, None);
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason == "manual note only")
        );
    }

    #[test]
    fn verify_many_uses_snapshot_per_claim() {
        let claim_a = claim("REQ-auth-001", "sha256:claim-a");
        let claim_b = claim("REQ-auth-002", "sha256:claim-b");
        let evidence = vec![
            hard_evidence(
                "EVID-000001",
                "REQ-auth-001",
                Verdict::Pass,
                "sha256:claim-a",
                current_artifacts(),
            ),
            hard_evidence(
                "EVID-000002",
                "REQ-auth-002",
                Verdict::Pass,
                "sha256:claim-b",
                BTreeMap::from([("src/session.rs".into(), "sha256:session".into())]),
            ),
        ];
        let snapshots = BTreeMap::from([
            (claim_a.id.clone(), current_artifacts()),
            (
                claim_b.id.clone(),
                BTreeMap::from([("src/session.rs".into(), "sha256:session".into())]),
            ),
        ]);

        let reports = verify_many(&[claim_a, claim_b], &snapshots, &evidence);

        assert_eq!(reports.len(), 2);
        assert!(
            reports
                .iter()
                .all(|report| report.status == ClaimStatus::Confirmed)
        );
    }
}
