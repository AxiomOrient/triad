use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::{ClaimId, EvidenceId, PatchId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub title: String,
    pub statement: String,
    pub examples: Vec<String>,
    pub invariants: Vec<String>,
    pub notes: Option<String>,
    pub revision: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceKind {
    Unit,
    Contract,
    Integration,
    Probe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: EvidenceId,
    pub claim_id: ClaimId,
    pub kind: EvidenceKind,
    pub verdict: Verdict,
    pub test_selector: Option<String>,
    pub command: String,
    pub covered_paths: Vec<Utf8PathBuf>,
    pub covered_digests: BTreeMap<Utf8PathBuf, String>,
    pub spec_revision: u32,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriftStatus {
    Healthy,
    NeedsCode,
    NeedsTest,
    NeedsSpec,
    Contradicted,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    pub claim_id: ClaimId,
    pub status: DriftStatus,
    pub reasons: Vec<String>,
    pub fresh_evidence_ids: Vec<EvidenceId>,
    pub pending_patch_id: Option<PatchId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PatchState {
    Pending,
    Applied,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchDraft {
    pub id: PatchId,
    pub claim_id: ClaimId,
    pub based_on_evidence: Vec<EvidenceId>,
    pub unified_diff: String,
    pub rationale: String,
    pub created_at: String,
    pub state: PatchState,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use camino::Utf8PathBuf;
    use serde_json::{from_str, from_value, json, to_string, to_value};

    use super::{
        Claim, DriftReport, DriftStatus, Evidence, EvidenceKind, PatchDraft, PatchState, Verdict,
    };
    use crate::{ClaimId, EvidenceId, PatchId};

    #[test]
    fn serde_tags_model_enums_use_kebab_case() {
        assert_eq!(
            to_string(&EvidenceKind::Integration).expect("enum should serialize"),
            "\"integration\""
        );
        assert_eq!(
            to_string(&Verdict::Unknown).expect("enum should serialize"),
            "\"unknown\""
        );
        assert_eq!(
            to_string(&DriftStatus::NeedsSpec).expect("enum should serialize"),
            "\"needs-spec\""
        );
        assert_eq!(
            to_string(&PatchState::Superseded).expect("enum should serialize"),
            "\"superseded\""
        );

        assert!(matches!(
            from_str::<EvidenceKind>("\"contract\"").expect("enum should deserialize"),
            EvidenceKind::Contract
        ));
        assert!(matches!(
            from_str::<Verdict>("\"fail\"").expect("enum should deserialize"),
            Verdict::Fail
        ));
        assert!(matches!(
            from_str::<DriftStatus>("\"needs-test\"").expect("enum should deserialize"),
            DriftStatus::NeedsTest
        ));
        assert!(matches!(
            from_str::<PatchState>("\"applied\"").expect("enum should deserialize"),
            PatchState::Applied
        ));
    }

    #[test]
    fn model_contract_structs_roundtrip_without_io() {
        let claim_id = ClaimId::new("REQ-auth-001").expect("claim id should parse");
        let evidence_id = EvidenceId::new("EVID-000001").expect("evidence id should parse");
        let patch_id = PatchId::new("PATCH-000001").expect("patch id should parse");

        let claim = Claim {
            id: claim_id.clone(),
            title: "Login success".into(),
            statement: "System grants access with valid credentials.".into(),
            examples: vec!["Given valid credentials, login succeeds.".into()],
            invariants: vec!["Successful login returns a session.".into()],
            notes: Some("OAuth handled elsewhere.".into()),
            revision: 7,
        };
        let claim_json = to_value(&claim).expect("claim should serialize");
        assert_eq!(claim_json["id"], "REQ-auth-001");
        assert_eq!(claim_json["revision"], 7);
        let claim_roundtrip: Claim = from_value(claim_json).expect("claim should deserialize");
        assert_eq!(claim_roundtrip.id, claim.id);
        assert_eq!(claim_roundtrip.examples, claim.examples);
        assert_eq!(claim_roundtrip.notes, claim.notes);

        let mut covered_digests = BTreeMap::new();
        covered_digests.insert(Utf8PathBuf::from("src/auth.rs"), "sha256:abc123".into());
        let evidence = Evidence {
            id: evidence_id.clone(),
            claim_id: claim_id.clone(),
            kind: EvidenceKind::Integration,
            verdict: Verdict::Pass,
            test_selector: Some("auth::login_success".into()),
            command: "cargo test auth::login_success".into(),
            covered_paths: vec![Utf8PathBuf::from("src/auth.rs")],
            covered_digests,
            spec_revision: 7,
            created_at: "2026-03-10T10:00:00Z".into(),
        };
        let evidence_json = to_value(&evidence).expect("evidence should serialize");
        assert_eq!(evidence_json["kind"], "integration");
        assert_eq!(evidence_json["covered_paths"], json!(["src/auth.rs"]));
        let evidence_roundtrip: Evidence =
            from_value(evidence_json).expect("evidence should deserialize");
        assert_eq!(evidence_roundtrip.id, evidence.id);
        assert_eq!(evidence_roundtrip.claim_id, evidence.claim_id);
        assert_eq!(evidence_roundtrip.covered_paths, evidence.covered_paths);
        assert_eq!(evidence_roundtrip.covered_digests, evidence.covered_digests);

        let drift = DriftReport {
            claim_id: claim_id.clone(),
            status: DriftStatus::Healthy,
            reasons: vec!["fresh pass evidence exists".into()],
            fresh_evidence_ids: vec![evidence_id.clone()],
            pending_patch_id: Some(patch_id.clone()),
        };
        let drift_json = to_value(&drift).expect("drift report should serialize");
        assert_eq!(drift_json["status"], "healthy");
        assert_eq!(drift_json["fresh_evidence_ids"], json!(["EVID-000001"]));
        let drift_roundtrip: DriftReport =
            from_value(drift_json).expect("drift report should deserialize");
        assert_eq!(drift_roundtrip.claim_id, drift.claim_id);
        assert_eq!(drift_roundtrip.pending_patch_id, drift.pending_patch_id);

        let patch = PatchDraft {
            id: patch_id,
            claim_id,
            based_on_evidence: vec![evidence_id],
            unified_diff: "--- a/spec\n+++ b/spec\n".into(),
            rationale: "Behavior diverged from current spec.".into(),
            created_at: "2026-03-10T10:05:00Z".into(),
            state: PatchState::Pending,
        };
        let patch_json = to_value(&patch).expect("patch draft should serialize");
        assert_eq!(patch_json["id"], "PATCH-000001");
        assert_eq!(patch_json["based_on_evidence"], json!(["EVID-000001"]));
        assert_eq!(patch_json["state"], "pending");
        let patch_roundtrip: PatchDraft =
            from_value(patch_json).expect("patch draft should deserialize");
        assert_eq!(patch_roundtrip.id, patch.id);
        assert_eq!(patch_roundtrip.claim_id, patch.claim_id);
        assert_eq!(patch_roundtrip.rationale, patch.rationale);
        assert_eq!(patch_roundtrip.state, patch.state);
    }
}
