use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ClaimId, EvidenceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub title: String,
    pub statement: String,
    pub examples: Vec<String>,
    pub invariants: Vec<String>,
    pub notes: Option<String>,
    pub revision_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    Hard,
    Advisory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceKind {
    Test,
    Analysis,
    Replay,
    Benchmark,
    Human,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub actor: String,
    pub runtime: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub workflow_id: Option<String>,
    pub commit: Option<String>,
    pub environment_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: EvidenceId,
    pub claim_id: ClaimId,
    pub class: EvidenceClass,
    pub kind: EvidenceKind,
    pub verdict: Verdict,
    pub verifier: String,
    pub claim_revision_digest: String,
    pub artifact_digests: BTreeMap<String, String>,
    pub command: Option<String>,
    pub locator: Option<String>,
    pub summary: Option<String>,
    pub provenance: Provenance,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimStatus {
    Confirmed,
    Contradicted,
    Blocked,
    Stale,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimReport {
    pub claim_id: ClaimId,
    pub status: ClaimStatus,
    pub reasons: Vec<String>,
    pub fresh_evidence_ids: Vec<EvidenceId>,
    pub stale_evidence_ids: Vec<EvidenceId>,
    pub advisory_evidence_ids: Vec<EvidenceId>,
    pub strongest_verdict: Option<Verdict>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{from_str, from_value, json, to_string, to_value};

    use super::{
        Claim, ClaimReport, ClaimStatus, Evidence, EvidenceClass, EvidenceKind, Provenance, Verdict,
    };
    use crate::{ClaimId, EvidenceId};

    #[test]
    fn serde_tags_model_enums_use_kebab_case() {
        assert_eq!(
            to_string(&EvidenceClass::Hard).expect("enum should serialize"),
            "\"hard\""
        );
        assert_eq!(
            to_string(&EvidenceKind::Benchmark).expect("enum should serialize"),
            "\"benchmark\""
        );
        assert_eq!(
            to_string(&Verdict::Unknown).expect("enum should serialize"),
            "\"unknown\""
        );
        assert_eq!(
            to_string(&ClaimStatus::Unsupported).expect("enum should serialize"),
            "\"unsupported\""
        );

        assert!(matches!(
            from_str::<EvidenceClass>("\"advisory\"").expect("enum should deserialize"),
            EvidenceClass::Advisory
        ));
        assert!(matches!(
            from_str::<EvidenceKind>("\"analysis\"").expect("enum should deserialize"),
            EvidenceKind::Analysis
        ));
        assert!(matches!(
            from_str::<ClaimStatus>("\"stale\"").expect("enum should deserialize"),
            ClaimStatus::Stale
        ));
    }

    #[test]
    fn model_contract_structs_roundtrip_without_io() {
        let claim_id = ClaimId::new("REQ-auth-001").expect("claim id should parse");
        let evidence_id = EvidenceId::new("EVID-000001").expect("evidence id should parse");

        let claim = Claim {
            id: claim_id.clone(),
            title: "Login success".into(),
            statement: "System grants access with valid credentials.".into(),
            examples: vec!["Given valid credentials, login succeeds.".into()],
            invariants: vec!["Successful login returns a session.".into()],
            notes: Some("OAuth handled elsewhere.".into()),
            revision_digest: "sha256:claim-digest".into(),
        };
        let claim_json = to_value(&claim).expect("claim should serialize");
        assert_eq!(claim_json["id"], "REQ-auth-001");
        assert_eq!(claim_json["revision_digest"], "sha256:claim-digest");
        let claim_roundtrip: Claim = from_value(claim_json).expect("claim should deserialize");
        assert_eq!(claim_roundtrip, claim);

        let mut artifact_digests = BTreeMap::new();
        artifact_digests.insert("src/auth.rs".into(), "sha256:file-digest".into());
        let evidence = Evidence {
            id: evidence_id.clone(),
            claim_id: claim_id.clone(),
            class: EvidenceClass::Hard,
            kind: EvidenceKind::Test,
            verdict: Verdict::Pass,
            verifier: "cargo test".into(),
            claim_revision_digest: claim.revision_digest.clone(),
            artifact_digests,
            command: Some("cargo test auth::login_success".into()),
            locator: Some("auth::login_success".into()),
            summary: Some("fresh passing evidence".into()),
            provenance: Provenance {
                actor: "ci".into(),
                runtime: Some("cargo-test".into()),
                session_id: Some("session-1".into()),
                task_id: Some("TRI-RNW-10".into()),
                workflow_id: Some("renewal".into()),
                commit: Some("abc123".into()),
                environment_digest: Some("env:linux".into()),
            },
            created_at: "2026-03-10T10:00:00Z".into(),
        };
        let evidence_json = to_value(&evidence).expect("evidence should serialize");
        assert_eq!(evidence_json["class"], "hard");
        assert_eq!(
            evidence_json["artifact_digests"],
            json!({"src/auth.rs": "sha256:file-digest"})
        );
        let evidence_roundtrip: Evidence =
            from_value(evidence_json).expect("evidence should deserialize");
        assert_eq!(evidence_roundtrip, evidence);

        let report = ClaimReport {
            claim_id,
            status: ClaimStatus::Confirmed,
            reasons: vec!["fresh hard pass exists".into()],
            fresh_evidence_ids: vec![evidence_id.clone()],
            stale_evidence_ids: vec![],
            advisory_evidence_ids: vec![],
            strongest_verdict: Some(Verdict::Pass),
        };
        let report_json = to_value(&report).expect("report should serialize");
        assert_eq!(report_json["status"], "confirmed");
        assert_eq!(report_json["fresh_evidence_ids"], json!(["EVID-000001"]));
        let report_roundtrip: ClaimReport =
            from_value(report_json).expect("report should deserialize");
        assert_eq!(report_roundtrip, report);
    }
}
