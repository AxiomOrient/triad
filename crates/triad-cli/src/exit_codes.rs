use std::process::ExitCode;

use triad_core::error::TriadErrorKind;
use triad_core::{
    ApplyPatchReport, ClaimSummary, DriftReport, DriftStatus, NextAction, NextClaim, PatchId,
    RunClaimReport, StatusReport, TriadError, Verdict, VerifyReport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliExit {
    Success = 0,
    DriftDetected = 2,
    VerificationFailed = 3,
    PatchApprovalRequired = 4,
    InvalidInput = 5,
    InternalError = 7,
}

impl CliExit {
    pub(crate) fn as_exit_code(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

pub(crate) fn exit_code_for_next(next: &NextClaim) -> CliExit {
    match next.next_action {
        NextAction::Accept => CliExit::PatchApprovalRequired,
        _ => exit_code_for_drift_status(next.status, None),
    }
}

pub(crate) fn exit_code_for_work(report: &RunClaimReport) -> CliExit {
    if report.needs_patch {
        CliExit::PatchApprovalRequired
    } else {
        CliExit::Success
    }
}

pub(crate) fn exit_code_for_verify(report: &VerifyReport) -> CliExit {
    if matches!(report.verdict, Verdict::Fail) {
        return CliExit::VerificationFailed;
    }

    if report.pending_patch_id.is_some() {
        return CliExit::PatchApprovalRequired;
    }

    exit_code_for_drift_status(report.status_after_verify, report.pending_patch_id.as_ref())
}

pub(crate) fn exit_code_for_accept(report: &ApplyPatchReport) -> CliExit {
    if report.applied {
        CliExit::Success
    } else {
        CliExit::PatchApprovalRequired
    }
}

pub(crate) fn exit_code_for_status(report: &StatusReport) -> CliExit {
    exit_code_for_claim_summaries(&report.claims)
}

pub(crate) fn exit_code_for_claim_summaries(claims: &[ClaimSummary]) -> CliExit {
    if claims.iter().any(|claim| claim.pending_patch_id.is_some()) {
        return CliExit::PatchApprovalRequired;
    }

    if claims
        .iter()
        .any(|claim| !matches!(claim.status, DriftStatus::Healthy))
    {
        return CliExit::DriftDetected;
    }

    CliExit::Success
}

pub(crate) fn exit_code_for_drift(drift: &DriftReport) -> CliExit {
    exit_code_for_drift_status(drift.status, drift.pending_patch_id.as_ref())
}

pub(crate) fn exit_code_for_error(error: &anyhow::Error) -> CliExit {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<TriadError>())
        .map(|error| match error.kind() {
            TriadErrorKind::VerificationFailed => CliExit::VerificationFailed,
            TriadErrorKind::Config
            | TriadErrorKind::Parse
            | TriadErrorKind::InvalidState
            | TriadErrorKind::RuntimeBlocked
            | TriadErrorKind::PatchConflict => CliExit::InvalidInput,
            TriadErrorKind::Io | TriadErrorKind::Serialization => CliExit::InternalError,
        })
        .unwrap_or(CliExit::InternalError)
}

fn exit_code_for_drift_status(status: DriftStatus, pending_patch_id: Option<&PatchId>) -> CliExit {
    if pending_patch_id.is_some() || matches!(status, DriftStatus::NeedsSpec) {
        return CliExit::PatchApprovalRequired;
    }

    if matches!(status, DriftStatus::Healthy) {
        CliExit::Success
    } else {
        CliExit::DriftDetected
    }
}
