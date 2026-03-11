use std::path::Path;

use triad_core::{
    ClaimId, ClaimSummary, DriftReport, Evidence, NextAction, PatchId, StatusSummary, TriadError,
    Verdict,
};

use crate::claims::{ParsedClaimCatalog, claim_issue_for_id};
use crate::storage::pending_patch_ids_by_claim;
use crate::verify_exec::{latest_relevant_evidence, relevant_evidence_from_slice};
use crate::{LocalTriad, read_evidence};

pub(crate) fn next_action_for_status(status: triad_core::DriftStatus) -> NextAction {
    match status {
        triad_core::DriftStatus::Healthy => NextAction::Status,
        triad_core::DriftStatus::NeedsCode | triad_core::DriftStatus::Contradicted => {
            NextAction::Work
        }
        triad_core::DriftStatus::NeedsTest => NextAction::Verify,
        triad_core::DriftStatus::NeedsSpec => NextAction::Accept,
        triad_core::DriftStatus::Blocked => NextAction::Status,
    }
}

pub(crate) fn status_priority(status: triad_core::DriftStatus) -> Option<u8> {
    match status {
        triad_core::DriftStatus::Contradicted => Some(0),
        triad_core::DriftStatus::NeedsTest => Some(1),
        triad_core::DriftStatus::NeedsCode => Some(2),
        triad_core::DriftStatus::NeedsSpec => Some(3),
        triad_core::DriftStatus::Blocked => Some(4),
        triad_core::DriftStatus::Healthy => None,
    }
}

pub(crate) fn compute_drift(
    repo_root: &Path,
    id: &ClaimId,
    all_evidence: &[Evidence],
    pending_patch_id: Option<PatchId>,
) -> Result<DriftReport, TriadError> {
    let relevant = relevant_evidence_from_slice(repo_root, all_evidence, id)?;
    let fresh_evidence_ids = [
        relevant.pass.as_ref(),
        relevant.fail.as_ref(),
        relevant.unknown.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|evidence| evidence.id.clone())
    .collect::<Vec<_>>();

    if let Some(latest) = latest_relevant_evidence(&relevant) {
        return match latest.verdict {
            Verdict::Pass => Ok(DriftReport {
                claim_id: id.clone(),
                status: if pending_patch_id.is_some() {
                    triad_core::DriftStatus::NeedsSpec
                } else {
                    triad_core::DriftStatus::Healthy
                },
                reasons: vec![if pending_patch_id.is_some() {
                    "fresh pass evidence exists and a pending patch is present".to_string()
                } else {
                    "fresh pass evidence exists and no pending patch is present".to_string()
                }],
                fresh_evidence_ids,
                pending_patch_id,
            }),
            Verdict::Fail => Ok(DriftReport {
                claim_id: id.clone(),
                status: triad_core::DriftStatus::Contradicted,
                reasons: vec!["latest fresh evidence is failing".to_string()],
                fresh_evidence_ids,
                pending_patch_id,
            }),
            Verdict::Unknown => Ok(DriftReport {
                claim_id: id.clone(),
                status: triad_core::DriftStatus::Blocked,
                reasons: vec!["latest fresh evidence is unknown".to_string()],
                fresh_evidence_ids,
                pending_patch_id,
            }),
        };
    }

    let has_impl_paths = all_evidence
        .iter()
        .any(|evidence| &evidence.claim_id == id && !evidence.covered_paths.is_empty());
    let status = if has_impl_paths {
        triad_core::DriftStatus::NeedsTest
    } else {
        triad_core::DriftStatus::NeedsCode
    };
    let reason = match status {
        triad_core::DriftStatus::NeedsTest => {
            "no fresh evidence exists and implementation paths were previously observed"
        }
        triad_core::DriftStatus::NeedsCode => {
            "no fresh evidence exists and no implementation paths were observed"
        }
        _ => unreachable!("status is restricted above"),
    };

    Ok(DriftReport {
        claim_id: id.clone(),
        status,
        reasons: vec![reason.to_string()],
        fresh_evidence_ids,
        pending_patch_id,
    })
}

pub(crate) fn collect_claim_summaries(
    triad: &LocalTriad,
    filter: Option<&ClaimId>,
) -> Result<Vec<ClaimSummary>, TriadError> {
    let all_evidence = read_evidence(triad.config.paths.evidence_file.as_std_path())?;
    let pending_by_claim = pending_patch_ids_by_claim(triad.config.paths.patch_dir.as_std_path())?;
    let repo_root = triad.config.repo_root.as_std_path();
    let ParsedClaimCatalog { claims, issues } = triad.parsed_claim_catalog()?;
    let mut summaries = claims
        .into_iter()
        .filter(|claim| filter.is_none_or(|id| claim.id == *id))
        .map(|claim| {
            let pending_patch_id = pending_by_claim.get(&claim.id).cloned();
            let drift = compute_drift(repo_root, &claim.id, &all_evidence, pending_patch_id)?;
            Ok(ClaimSummary {
                claim_id: claim.id,
                title: claim.title,
                status: drift.status,
                revision: claim.revision,
                pending_patch_id: drift.pending_patch_id,
            })
        })
        .collect::<Result<Vec<_>, TriadError>>()?;

    summaries.sort_by(|left, right| left.claim_id.as_str().cmp(right.claim_id.as_str()));

    if let Some(filter_id) = filter {
        if summaries.is_empty() {
            if let Some(issue) = claim_issue_for_id(&issues, filter_id) {
                return Err(TriadError::InvalidState(issue.diagnostic.clone()));
            }
            return Err(TriadError::InvalidState(format!(
                "claim not found: {filter_id}"
            )));
        }
    }

    Ok(summaries)
}

pub(crate) fn summarize_claim_statuses(claims: &[ClaimSummary]) -> StatusSummary {
    let mut summary = StatusSummary {
        healthy: 0,
        needs_code: 0,
        needs_test: 0,
        needs_spec: 0,
        contradicted: 0,
        blocked: 0,
    };

    for claim in claims {
        match claim.status {
            triad_core::DriftStatus::Healthy => summary.healthy += 1,
            triad_core::DriftStatus::NeedsCode => summary.needs_code += 1,
            triad_core::DriftStatus::NeedsTest => summary.needs_test += 1,
            triad_core::DriftStatus::NeedsSpec => summary.needs_spec += 1,
            triad_core::DriftStatus::Contradicted => summary.contradicted += 1,
            triad_core::DriftStatus::Blocked => summary.blocked += 1,
        }
    }

    summary
}
