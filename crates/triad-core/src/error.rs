use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriadErrorKind {
    Config,
    Parse,
    Io,
    InvalidState,
    RuntimeBlocked,
    VerificationFailed,
    PatchConflict,
    Serialization,
}

impl TriadErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Parse => "parse",
            Self::Io => "io",
            Self::InvalidState => "invalid-state",
            Self::RuntimeBlocked => "runtime-blocked",
            Self::VerificationFailed => "verification-failed",
            Self::PatchConflict => "patch-conflict",
            Self::Serialization => "serialization",
        }
    }
}

impl fmt::Display for TriadErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum TriadError {
    #[error("config error: {0}")]
    Config(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("runtime blocked: {0}")]
    RuntimeBlocked(String),
    #[error("verification failed: {0}")]
    VerificationFailed(String),
    #[error("patch conflict: {0}")]
    PatchConflict(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl TriadError {
    pub fn kind(&self) -> TriadErrorKind {
        match self {
            Self::Config(_) => TriadErrorKind::Config,
            Self::Parse(_) => TriadErrorKind::Parse,
            Self::Io(_) => TriadErrorKind::Io,
            Self::InvalidState(_) => TriadErrorKind::InvalidState,
            Self::RuntimeBlocked(_) => TriadErrorKind::RuntimeBlocked,
            Self::VerificationFailed(_) => TriadErrorKind::VerificationFailed,
            Self::PatchConflict(_) => TriadErrorKind::PatchConflict,
            Self::Serialization(_) => TriadErrorKind::Serialization,
        }
    }

    pub fn invalid_id(kind: &str, value: &str) -> Self {
        Self::Parse(format!("invalid {kind}: {value}"))
    }

    pub fn config_field(field: &str, detail: &str) -> Self {
        Self::Config(format!("invalid config {field}: {detail}"))
    }

    pub fn patch_conflict(patch_id: &str, detail: &str) -> Self {
        Self::PatchConflict(format!("{patch_id}: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{TriadError, TriadErrorKind};

    #[test]
    fn error_mapping_covers_documented_categories() {
        let cases = [
            (
                TriadError::Config("bad config".into()),
                TriadErrorKind::Config,
            ),
            (TriadError::Parse("bad parse".into()), TriadErrorKind::Parse),
            (TriadError::Io("disk error".into()), TriadErrorKind::Io),
            (
                TriadError::InvalidState("invalid state".into()),
                TriadErrorKind::InvalidState,
            ),
            (
                TriadError::RuntimeBlocked("blocked tool".into()),
                TriadErrorKind::RuntimeBlocked,
            ),
            (
                TriadError::VerificationFailed("tests failed".into()),
                TriadErrorKind::VerificationFailed,
            ),
            (
                TriadError::PatchConflict("merge conflict".into()),
                TriadErrorKind::PatchConflict,
            ),
            (
                TriadError::Serialization("bad json".into()),
                TriadErrorKind::Serialization,
            ),
        ];

        for (error, expected_kind) in cases {
            assert_eq!(error.kind(), expected_kind);
            assert_eq!(error.kind().as_str(), expected_kind.as_str());
        }
    }

    #[test]
    fn error_mapping_invalid_id_is_parse() {
        let error = TriadError::invalid_id("claim id", "REQ-auth-01");

        assert_eq!(error.kind(), TriadErrorKind::Parse);
        assert_eq!(error.kind().as_str(), "parse");
        assert_eq!(
            error.to_string(),
            "parse error: invalid claim id: REQ-auth-01"
        );
    }

    #[test]
    fn error_mapping_config_field_is_config() {
        let error = TriadError::config_field("paths.claim_dir", "must not be empty");

        assert_eq!(error.kind(), TriadErrorKind::Config);
        assert_eq!(error.kind().as_str(), "config");
        assert_eq!(
            error.to_string(),
            "config error: invalid config paths.claim_dir: must not be empty"
        );
    }

    #[test]
    fn error_mapping_patch_conflict_helper_is_patch_conflict() {
        let error = TriadError::patch_conflict("PATCH-000001", "claim file no longer matches");

        assert_eq!(error.kind(), TriadErrorKind::PatchConflict);
        assert_eq!(error.kind().as_str(), "patch-conflict");
        assert_eq!(
            error.to_string(),
            "patch conflict: PATCH-000001: claim file no longer matches"
        );
    }
}
