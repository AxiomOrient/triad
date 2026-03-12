use std::process::ExitCode;

use triad_core::{ClaimReport, ClaimStatus, TriadError, error::TriadErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliExit {
    Success = 0,
    Failure = 2,
    InvalidInput = 5,
    InternalError = 7,
}

impl CliExit {
    pub(crate) fn as_exit_code(self) -> ExitCode {
        ExitCode::from(self as u8)
    }

    pub(crate) fn for_claim_report(report: &ClaimReport) -> Self {
        match report.status {
            ClaimStatus::Contradicted | ClaimStatus::Blocked => Self::Failure,
            _ => Self::Success,
        }
    }

    pub(crate) fn for_reports(reports: &[ClaimReport]) -> Self {
        if reports.iter().any(|report| {
            matches!(
                report.status,
                ClaimStatus::Contradicted | ClaimStatus::Blocked
            )
        }) {
            Self::Failure
        } else {
            Self::Success
        }
    }
}

pub(crate) fn exit_code_for_error(error: &anyhow::Error) -> CliExit {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<TriadError>())
        .map(|error| match error.kind() {
            TriadErrorKind::Config | TriadErrorKind::Parse | TriadErrorKind::InvalidState => {
                CliExit::InvalidInput
            }
            TriadErrorKind::Io
            | TriadErrorKind::Serialization
            | TriadErrorKind::VerificationFailed => CliExit::InternalError,
        })
        .unwrap_or(CliExit::InternalError)
}
