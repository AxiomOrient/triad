use serde::{Deserialize, Serialize};

use crate::{
    Claim, ClaimId, DriftReport, DriftStatus, EvidenceId, PatchId, RunId, TriadError, Verdict,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestReport {
    pub claim_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimSummary {
    pub claim_id: ClaimId,
    pub title: String,
    pub status: DriftStatus,
    pub revision: u32,
    pub pending_patch_id: Option<PatchId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimBundle {
    pub claim: Claim,
    pub drift: DriftReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextClaim {
    pub claim_id: ClaimId,
    pub status: DriftStatus,
    pub reason: String,
    pub next_action: NextAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NextAction {
    Work,
    Verify,
    Accept,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunClaimRequest {
    pub claim_id: ClaimId,
    pub dry_run: bool,
    pub model: Option<String>,
    pub effort: Option<ReasoningLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunClaimReport {
    pub run_id: RunId,
    pub claim_id: ClaimId,
    pub summary: String,
    pub changed_paths: Vec<String>,
    pub suggested_test_selectors: Vec<String>,
    pub blocked_actions: Vec<String>,
    pub needs_patch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifyLayer {
    Unit,
    Contract,
    Integration,
    Probe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRequest {
    pub claim_id: ClaimId,
    pub layers: Vec<VerifyLayer>,
    pub full_workspace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub claim_id: ClaimId,
    pub verdict: Verdict,
    pub layers: Vec<VerifyLayer>,
    pub full_workspace: bool,
    pub evidence_ids: Vec<EvidenceId>,
    pub status_after_verify: DriftStatus,
    pub pending_patch_id: Option<PatchId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposePatchReport {
    pub patch_id: PatchId,
    pub claim_id: ClaimId,
    pub based_on_evidence: Vec<EvidenceId>,
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchReport {
    pub patch_id: PatchId,
    pub claim_id: ClaimId,
    pub applied: bool,
    pub new_revision: u32,
    pub followup_action: NextAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSummary {
    pub healthy: u32,
    pub needs_code: u32,
    pub needs_test: u32,
    pub needs_spec: u32,
    pub contradicted: u32,
    pub blocked: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    pub summary: StatusSummary,
    pub claims: Vec<ClaimSummary>,
}

pub trait TriadApi {
    fn ingest_spec(&self) -> Result<IngestReport, TriadError>;
    fn list_claims(&self) -> Result<Vec<ClaimSummary>, TriadError>;
    fn get_claim(&self, id: &ClaimId) -> Result<ClaimBundle, TriadError>;
    fn next_claim(&self) -> Result<NextClaim, TriadError>;
    fn detect_drift(&self, id: &ClaimId) -> Result<DriftReport, TriadError>;
    fn run_claim(&self, req: RunClaimRequest) -> Result<RunClaimReport, TriadError>;
    fn verify_claim(&self, req: VerifyRequest) -> Result<VerifyReport, TriadError>;
    fn propose_patch(&self, id: &ClaimId) -> Result<ProposePatchReport, TriadError>;
    fn apply_patch(&self, id: &PatchId) -> Result<ApplyPatchReport, TriadError>;
    fn status(&self, claim: Option<&ClaimId>) -> Result<StatusReport, TriadError>;
}

#[cfg(test)]
mod tests {
    use serde_json::{from_str, from_value, json, to_string, to_value};

    use super::{
        ApplyPatchReport, ClaimSummary, NextAction, NextClaim, ProposePatchReport, ReasoningLevel,
        RunClaimReport, RunClaimRequest, StatusReport, StatusSummary, VerifyLayer, VerifyReport,
        VerifyRequest,
    };
    use crate::{ClaimId, DriftStatus, EvidenceId, PatchId, Verdict};

    #[test]
    fn serde_tags_api_enums_use_kebab_case() {
        assert_eq!(
            to_string(&NextAction::Verify).expect("enum should serialize"),
            "\"verify\""
        );
        assert_eq!(
            to_string(&VerifyLayer::Integration).expect("enum should serialize"),
            "\"integration\""
        );

        assert_eq!(
            from_str::<NextAction>("\"accept\"").expect("enum should deserialize"),
            NextAction::Accept
        );
        assert_eq!(
            from_str::<VerifyLayer>("\"probe\"").expect("enum should deserialize"),
            VerifyLayer::Probe
        );
    }

    #[test]
    fn api_contract_reports_cover_schema_fields() {
        let claim_id = ClaimId::new("REQ-auth-001").expect("claim id should parse");
        let evidence_id = EvidenceId::new("EVID-000001").expect("evidence id should parse");
        let patch_id = PatchId::new("PATCH-000001").expect("patch id should parse");
        let run_id = crate::RunId::new("RUN-000001").expect("run id should parse");

        let next_claim = to_value(NextClaim {
            claim_id: claim_id.clone(),
            status: DriftStatus::NeedsTest,
            reason: "fresh verify required".into(),
            next_action: NextAction::Verify,
        })
        .expect("next claim should serialize");
        assert_eq!(
            next_claim,
            json!({
                "claim_id": "REQ-auth-001",
                "status": "needs-test",
                "reason": "fresh verify required",
                "next_action": "verify"
            })
        );

        let claim_summary = to_value(ClaimSummary {
            claim_id: claim_id.clone(),
            title: "Login success".into(),
            status: DriftStatus::Healthy,
            revision: 3,
            pending_patch_id: Some(patch_id.clone()),
        })
        .expect("claim summary should serialize");
        assert_eq!(claim_summary["claim_id"], "REQ-auth-001");
        assert_eq!(claim_summary["status"], "healthy");
        assert_eq!(claim_summary["pending_patch_id"], "PATCH-000001");

        let run_request = to_value(RunClaimRequest {
            claim_id: claim_id.clone(),
            dry_run: true,
            model: Some("gpt-5-codex".into()),
            effort: Some(ReasoningLevel::Medium),
        })
        .expect("run request should serialize");
        assert_eq!(run_request["claim_id"], "REQ-auth-001");
        assert_eq!(run_request["dry_run"], true);
        assert_eq!(run_request["model"], "gpt-5-codex");
        assert_eq!(run_request["effort"], "medium");

        let run_report = to_value(RunClaimReport {
            run_id,
            claim_id: claim_id.clone(),
            summary: "updated auth tests".into(),
            changed_paths: vec!["src/auth.rs".into()],
            suggested_test_selectors: vec!["auth::login_success".into()],
            blocked_actions: vec!["spec write".into()],
            needs_patch: false,
        })
        .expect("run report should serialize");
        assert_eq!(run_report["run_id"], "RUN-000001");
        assert_eq!(run_report["claim_id"], "REQ-auth-001");
        assert_eq!(run_report["needs_patch"], false);

        let verify_report = to_value(VerifyReport {
            claim_id: claim_id.clone(),
            verdict: Verdict::Pass,
            layers: vec![VerifyLayer::Unit, VerifyLayer::Integration],
            full_workspace: true,
            evidence_ids: vec![evidence_id.clone()],
            status_after_verify: DriftStatus::Healthy,
            pending_patch_id: None,
        })
        .expect("verify report should serialize");
        assert_eq!(verify_report["claim_id"], "REQ-auth-001");
        assert_eq!(verify_report["verdict"], "pass");
        assert_eq!(verify_report["layers"], json!(["unit", "integration"]));
        assert_eq!(verify_report["full_workspace"], true);
        assert_eq!(verify_report["evidence_ids"], json!(["EVID-000001"]));
        assert_eq!(verify_report["status_after_verify"], "healthy");

        let patch_report = to_value(ProposePatchReport {
            patch_id: patch_id.clone(),
            claim_id: claim_id.clone(),
            based_on_evidence: vec![evidence_id],
            path: "spec/claims/REQ-auth-001.md".into(),
            reason: "verified behavior changed".into(),
        })
        .expect("patch report should serialize");
        assert_eq!(patch_report["patch_id"], "PATCH-000001");
        assert_eq!(patch_report["claim_id"], "REQ-auth-001");
        assert_eq!(patch_report["based_on_evidence"], json!(["EVID-000001"]));
        assert_eq!(patch_report["path"], "spec/claims/REQ-auth-001.md");
        assert_eq!(patch_report["reason"], "verified behavior changed");

        let apply_report = to_value(ApplyPatchReport {
            patch_id,
            claim_id: claim_id.clone(),
            applied: true,
            new_revision: 4,
            followup_action: NextAction::Verify,
        })
        .expect("apply report should serialize");
        assert_eq!(apply_report["patch_id"], "PATCH-000001");
        assert_eq!(apply_report["followup_action"], "verify");

        let status_report = to_value(StatusReport {
            summary: StatusSummary {
                healthy: 1,
                needs_code: 2,
                needs_test: 3,
                needs_spec: 4,
                contradicted: 5,
                blocked: 6,
            },
            claims: vec![],
        })
        .expect("status report should serialize");
        assert_eq!(status_report["summary"]["needs_code"], 2);
        assert_eq!(status_report["summary"]["needs_spec"], 4);
        assert_eq!(status_report["claims"], json!([]));
    }

    #[test]
    fn api_contract_requests_and_reports_roundtrip_without_io() {
        let run_request: RunClaimRequest = from_value(json!({
            "claim_id": "REQ-auth-001",
            "dry_run": false,
            "model": "gpt-5-codex",
            "effort": "high"
        }))
        .expect("run request should deserialize");
        assert_eq!(run_request.claim_id.as_str(), "REQ-auth-001");
        assert!(!run_request.dry_run);
        assert_eq!(run_request.model.as_deref(), Some("gpt-5-codex"));
        assert_eq!(run_request.effort, Some(ReasoningLevel::High));

        let verify_request: VerifyRequest = from_value(json!({
            "claim_id": "REQ-auth-001",
            "layers": ["unit", "probe"],
            "full_workspace": true
        }))
        .expect("verify request should deserialize");
        assert_eq!(verify_request.claim_id.as_str(), "REQ-auth-001");
        assert_eq!(
            verify_request.layers,
            vec![VerifyLayer::Unit, VerifyLayer::Probe]
        );
        assert!(verify_request.full_workspace);

        let patch_report: ProposePatchReport = from_value(json!({
            "patch_id": "PATCH-000001",
            "claim_id": "REQ-auth-001",
            "based_on_evidence": ["EVID-000001"],
            "path": "spec/claims/REQ-auth-001.md",
            "reason": "verified behavior changed"
        }))
        .expect("patch report should deserialize");
        assert_eq!(patch_report.patch_id.as_str(), "PATCH-000001");
        assert_eq!(patch_report.claim_id.as_str(), "REQ-auth-001");
        assert_eq!(patch_report.based_on_evidence.len(), 1);
        assert_eq!(patch_report.based_on_evidence[0].as_str(), "EVID-000001");

        let status_report: StatusReport = from_value(json!({
            "summary": {
                "healthy": 1,
                "needs_code": 0,
                "needs_test": 2,
                "needs_spec": 3,
                "contradicted": 0,
                "blocked": 1
            },
            "claims": [{
                "claim_id": "REQ-auth-001",
                "title": "Login success",
                "status": "needs-test",
                "revision": 2,
                "pending_patch_id": null
            }]
        }))
        .expect("status report should deserialize");
        assert_eq!(status_report.summary.needs_test, 2);
        assert_eq!(status_report.claims.len(), 1);
        assert_eq!(status_report.claims[0].claim_id.as_str(), "REQ-auth-001");
        assert_eq!(status_report.claims[0].status, DriftStatus::NeedsTest);
    }
}
