mod agent_runtime;
mod claims;
mod drift;
mod fs_support;
mod patching;
mod repo_support;
mod run_exec;
mod run_result;
mod runtime_config;
mod scaffold;
mod storage;
mod verify_exec;
mod verify_plan;
mod work_contract;
mod work_guardrails;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use triad_config::{CanonicalTriadConfig, discover_repo_root};
use triad_core::{
    ApplyPatchReport, Claim, ClaimBundle, ClaimId, ClaimSummary, DriftReport, Evidence, EvidenceId,
    IngestReport, NextClaim, PatchDraft, PatchId, PatchState, ProposePatchReport, RunClaimReport,
    RunClaimRequest, RunId, StatusReport, TriadApi, TriadError, VerifyReport, VerifyRequest,
};

use crate::agent_runtime::{HostProcessRunner, RunProfile, SessionConfig};
use crate::claims::ParsedClaimCatalog;
use crate::claims::{
    claim_md_path, discover_claim_file_paths, no_valid_claims_error, parse_claim_catalog,
    parse_claim_file, parsed_claim_by_id_or_issue,
};
use crate::drift::{
    collect_claim_summaries, compute_drift, next_action_for_status, status_priority,
    summarize_claim_statuses,
};
use crate::patching::{
    apply_patch_with_runner, deterministic_mismatch_for_claim, minimal_claim_diff,
    proposed_claim_for_mismatch,
};
use crate::repo_support::{patch_diff_path, repo_relative_utf8};
use crate::run_exec::run_claim_with_backend_adapter;
use crate::runtime_config::{
    run_profile_from_triad, session_config_from_triad, session_config_from_triad_in_root,
};
use crate::scaffold::init_scaffold as init_runtime_scaffold;
use crate::storage::{
    append_evidence, latest_pending_patch_id, next_evidence_id, next_patch_id, next_run_id,
    pending_patch_id_for_claim, pending_patch_ids_by_claim, read_evidence, read_json_file,
    read_patch_draft, read_run_record, read_run_records, store_patch_draft, store_run_record,
};
pub use crate::verify_exec::RelevantEvidenceSet;
use crate::verify_exec::{
    ProcessCommandRunner, VerifyCommandRunner, covered_path_digests, current_timestamp_string,
    evidence_is_fresh, execute_verify_commands_with_runner, relevant_evidence_for_claim,
    verify_claim_with_runner,
};
pub use crate::verify_exec::{VerifyCommandExecution, VerifyCommandPlan};
use crate::verify_plan::{
    default_verify_request, plan_verify_commands, resolve_targeted_selectors,
};
pub use crate::work_contract::WorkPromptEnvelope;
use crate::work_contract::work_prompt_envelope;
use crate::work_guardrails::build_work_guardrails;
pub use crate::work_guardrails::{WorkGuardrails, WorkToolUse};

/// Local-first runtime facade.
///
/// Implementation intent:
/// - load strict markdown claims from spec/claims
/// - run one-shot agent backends inside staged workspaces
/// - write evidence to .triad/evidence.ndjson
/// - generate patch drafts under .triad/patches
pub struct LocalTriad {
    pub config: CanonicalTriadConfig,
}

impl LocalTriad {
    pub fn new(config: CanonicalTriadConfig) -> Self {
        Self { config }
    }

    pub fn discover_repo_root(start: impl AsRef<Path>) -> Result<PathBuf, TriadError> {
        discover_repo_root(start)
    }

    pub fn init_scaffold(&self, force: bool) -> Result<(), TriadError> {
        init_runtime_scaffold(self, force)
    }

    fn claim_file_paths(&self) -> Result<Vec<Utf8PathBuf>, TriadError> {
        discover_claim_file_paths(self.config.paths.claim_dir.as_std_path())
    }

    pub(crate) fn parsed_claim_catalog(&self) -> Result<ParsedClaimCatalog, TriadError> {
        parse_claim_catalog(self.claim_file_paths()?)
    }

    pub fn claim_load_diagnostics(&self) -> Result<Vec<String>, TriadError> {
        Ok(self
            .parsed_claim_catalog()?
            .issues
            .into_iter()
            .map(|issue| issue.diagnostic)
            .collect())
    }

    pub fn next_evidence_id(&self) -> Result<EvidenceId, TriadError> {
        next_evidence_id(self.config.paths.evidence_file.as_std_path())
    }

    pub fn append_evidence(&self, evidence: &Evidence) -> Result<(), TriadError> {
        append_evidence(self.config.paths.evidence_file.as_std_path(), evidence)
    }

    pub fn read_evidence(&self) -> Result<Vec<Evidence>, TriadError> {
        read_evidence(self.config.paths.evidence_file.as_std_path())
    }

    pub fn covered_digests(
        &self,
        covered_paths: &[Utf8PathBuf],
    ) -> Result<BTreeMap<Utf8PathBuf, String>, TriadError> {
        covered_path_digests(self.config.repo_root.as_std_path(), covered_paths)
    }

    pub fn evidence_is_fresh(&self, evidence: &Evidence) -> Result<bool, TriadError> {
        evidence_is_fresh(self.config.repo_root.as_std_path(), evidence)
    }

    pub fn relevant_evidence_for_claim(
        &self,
        claim_id: &ClaimId,
    ) -> Result<RelevantEvidenceSet, TriadError> {
        relevant_evidence_for_claim(
            self.config.repo_root.as_std_path(),
            self.config.paths.evidence_file.as_std_path(),
            claim_id,
        )
    }

    pub fn store_patch_draft(&self, draft: &PatchDraft) -> Result<(), TriadError> {
        store_patch_draft(
            self.config.repo_root.as_std_path(),
            &self.config.paths.claim_dir,
            self.config.paths.patch_dir.as_std_path(),
            draft,
        )
    }

    pub fn read_patch_draft(&self, id: &PatchId) -> Result<PatchDraft, TriadError> {
        read_patch_draft(
            self.config.repo_root.as_std_path(),
            self.config.paths.patch_dir.as_std_path(),
            id,
        )
    }

    pub fn pending_patch_id_for_claim(
        &self,
        claim_id: &ClaimId,
    ) -> Result<Option<PatchId>, TriadError> {
        pending_patch_id_for_claim(self.config.paths.patch_dir.as_std_path(), claim_id)
    }

    pub fn latest_pending_patch_id(&self) -> Result<Option<PatchId>, TriadError> {
        latest_pending_patch_id(self.config.paths.patch_dir.as_std_path())
    }

    pub fn next_patch_id(&self) -> Result<PatchId, TriadError> {
        next_patch_id(self.config.paths.patch_dir.as_std_path())
    }

    pub fn next_run_id(&self) -> Result<RunId, TriadError> {
        next_run_id(self.config.paths.run_dir.as_std_path())
    }

    pub fn store_run_record(
        &self,
        report: &RunClaimReport,
        prompt_fingerprint: &str,
        runtime_metadata: &BTreeMap<String, String>,
    ) -> Result<(), TriadError> {
        store_run_record(
            self.config.paths.run_dir.as_std_path(),
            report,
            prompt_fingerprint,
            runtime_metadata,
        )
    }

    pub fn read_run_record(&self, id: &RunId) -> Result<RunRecord, TriadError> {
        read_run_record(self.config.paths.run_dir.as_std_path(), id)
    }

    pub fn minimal_claim_diff(
        &self,
        current: &Claim,
        proposed: &Claim,
    ) -> Result<String, TriadError> {
        minimal_claim_diff(
            &repo_relative_utf8(
                self.config.repo_root.as_std_path(),
                claim_md_path(&self.config.paths.claim_dir, &current.id).as_std_path(),
            )?,
            current,
            proposed,
        )
    }

    pub fn resolve_targeted_selectors(
        &self,
        claim_id: &ClaimId,
    ) -> Result<Vec<String>, TriadError> {
        resolve_targeted_selectors(self, claim_id)
    }

    pub fn plan_verify_commands(
        &self,
        req: &VerifyRequest,
    ) -> Result<Vec<VerifyCommandPlan>, TriadError> {
        plan_verify_commands(self, req)
    }

    pub fn execute_verify_commands(
        &self,
        req: &VerifyRequest,
    ) -> Result<Vec<VerifyCommandExecution>, TriadError> {
        execute_verify_commands_with_runner(self, req, &ProcessCommandRunner)
    }

    pub fn default_verify_request(
        &self,
        claim_id: ClaimId,
        with_probe: bool,
        full_workspace: bool,
    ) -> Result<VerifyRequest, TriadError> {
        default_verify_request(&self.config, claim_id, with_probe, full_workspace)
    }

    pub fn run_profile(&self) -> Result<RunProfile, TriadError> {
        run_profile_from_triad(&self.config)
    }

    pub fn session_config(&self) -> Result<SessionConfig, TriadError> {
        session_config_from_triad(&self.config)
    }

    pub fn work_prompt_envelope(
        &self,
        claim_id: &ClaimId,
    ) -> Result<WorkPromptEnvelope, TriadError> {
        work_prompt_envelope(self, claim_id)
    }

    pub fn work_guardrails(
        &self,
        claim_id: &ClaimId,
        allowed_write_roots: &[Utf8PathBuf],
    ) -> Result<WorkGuardrails, TriadError> {
        build_work_guardrails(self, claim_id, allowed_write_roots)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: RunId,
    pub claim_id: ClaimId,
    pub summary: String,
    pub changed_paths: Vec<String>,
    pub suggested_test_selectors: Vec<String>,
    pub blocked_actions: Vec<String>,
    pub needs_patch: bool,
    pub prompt_fingerprint: String,
    pub runtime_metadata: BTreeMap<String, String>,
}

impl TriadApi for LocalTriad {
    fn ingest_spec(&self) -> Result<IngestReport, TriadError> {
        Ok(IngestReport {
            claim_count: self
                .claim_file_paths()?
                .into_iter()
                .map(|path| parse_claim_file(&path))
                .collect::<Result<Vec<_>, _>>()?
                .len(),
        })
    }

    fn list_claims(&self) -> Result<Vec<ClaimSummary>, TriadError> {
        collect_claim_summaries(self, None)
    }

    fn get_claim(&self, id: &ClaimId) -> Result<ClaimBundle, TriadError> {
        let claim = parsed_claim_by_id_or_issue(self, id)?;
        let all_evidence = read_evidence(self.config.paths.evidence_file.as_std_path())?;
        let pending_patch_id = self.pending_patch_id_for_claim(id)?;
        let drift = compute_drift(
            self.config.repo_root.as_std_path(),
            id,
            &all_evidence,
            pending_patch_id,
        )?;
        Ok(ClaimBundle { claim, drift })
    }

    fn next_claim(&self) -> Result<NextClaim, TriadError> {
        let ParsedClaimCatalog { claims, issues } = self.parsed_claim_catalog()?;
        let all_evidence = read_evidence(self.config.paths.evidence_file.as_std_path())?;
        let pending_by_claim =
            pending_patch_ids_by_claim(self.config.paths.patch_dir.as_std_path())?;
        let repo_root = self.config.repo_root.as_std_path();
        let mut actionable = Vec::new();
        let mut healthy = Vec::new();

        for claim in claims {
            let pending_patch_id = pending_by_claim.get(&claim.id).cloned();
            let drift = compute_drift(repo_root, &claim.id, &all_evidence, pending_patch_id)?;
            let candidate = (claim.id.clone(), drift.status, drift.reasons.join("; "));

            if let Some(priority) = status_priority(drift.status) {
                actionable.push((priority, candidate));
            } else {
                healthy.push(candidate);
            }
        }

        let selected = if actionable.is_empty() {
            healthy
                .into_iter()
                .min_by(|left, right| left.0.as_str().cmp(right.0.as_str()))
                .ok_or_else(|| no_valid_claims_error(&issues))?
        } else {
            actionable
                .into_iter()
                .min_by(|left, right| {
                    left.0
                        .cmp(&right.0)
                        .then_with(|| left.1.0.as_str().cmp(right.1.0.as_str()))
                })
                .map(|(_, candidate)| candidate)
                .expect("actionable candidates are non-empty")
        };

        Ok(NextClaim {
            claim_id: selected.0.clone(),
            status: selected.1,
            reason: selected.2,
            next_action: next_action_for_status(selected.1),
        })
    }

    fn detect_drift(&self, id: &ClaimId) -> Result<DriftReport, TriadError> {
        // Validate claim exists.
        let _claim = parsed_claim_by_id_or_issue(self, id)?;
        let all_evidence = read_evidence(self.config.paths.evidence_file.as_std_path())?;
        let pending_patch_id = self.pending_patch_id_for_claim(id)?;
        compute_drift(
            self.config.repo_root.as_std_path(),
            id,
            &all_evidence,
            pending_patch_id,
        )
    }

    fn run_claim(&self, req: RunClaimRequest) -> Result<RunClaimReport, TriadError> {
        run_claim_with_backend_adapter(self, req, &HostProcessRunner)
    }

    fn verify_claim(&self, req: VerifyRequest) -> Result<VerifyReport, TriadError> {
        verify_claim_with_runner(self, req, &ProcessCommandRunner)
    }

    fn propose_patch(&self, id: &ClaimId) -> Result<ProposePatchReport, TriadError> {
        if let Some(pending) = self.pending_patch_id_for_claim(id)? {
            return Err(TriadError::InvalidState(format!(
                "pending patch already exists for {}: {}",
                id, pending
            )));
        }

        let mismatch = deterministic_mismatch_for_claim(self, id)?.ok_or_else(|| {
            TriadError::InvalidState(format!("no deterministic mismatch detected for {}", id))
        })?;
        let current = parsed_claim_by_id_or_issue(self, id)?;
        let proposed = proposed_claim_for_mismatch(&current, &mismatch);
        let patch_id = self.next_patch_id()?;
        let unified_diff = self.minimal_claim_diff(&current, &proposed)?;
        let created_at = current_timestamp_string()?;
        let draft = PatchDraft {
            id: patch_id.clone(),
            claim_id: mismatch.claim_id.clone(),
            based_on_evidence: mismatch.based_on_evidence.clone(),
            unified_diff,
            rationale: mismatch.reason.clone(),
            created_at,
            state: PatchState::Pending,
        };
        self.store_patch_draft(&draft)?;

        Ok(ProposePatchReport {
            patch_id: patch_id.clone(),
            claim_id: mismatch.claim_id,
            based_on_evidence: mismatch.based_on_evidence,
            path: repo_relative_utf8(
                self.config.repo_root.as_std_path(),
                &patch_diff_path(self.config.paths.patch_dir.as_std_path(), &patch_id),
            )?
            .into_string(),
            reason: mismatch.reason,
        })
    }

    fn apply_patch(&self, id: &PatchId) -> Result<ApplyPatchReport, TriadError> {
        apply_patch_with_runner(self, id, &ProcessCommandRunner)
    }

    fn status(&self, claim: Option<&ClaimId>) -> Result<StatusReport, TriadError> {
        let claims = collect_claim_summaries(self, claim)?;

        Ok(StatusReport {
            summary: summarize_claim_statuses(&claims),
            claims,
        })
    }
}

#[cfg(test)]
mod tests;
