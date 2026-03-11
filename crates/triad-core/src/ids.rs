use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::TriadError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClaimId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EvidenceId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PatchId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunId(String);

macro_rules! impl_id_type {
    ($name:ident, $kind:literal, $validator:ident) => {
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, TriadError> {
                let value = value.into();
                if $validator(&value) {
                    Ok(Self(value))
                } else {
                    Err(TriadError::invalid_id($kind, &value))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = TriadError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = TriadError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = TriadError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

impl_id_type!(ClaimId, "claim id", is_valid_claim_id);
impl_id_type!(EvidenceId, "evidence id", is_valid_evidence_id);
impl_id_type!(PatchId, "patch id", is_valid_patch_id);
impl_id_type!(RunId, "run id", is_valid_run_id);

impl EvidenceId {
    pub fn from_sequence(sequence: u32) -> Result<Self, TriadError> {
        numeric_id_from_sequence("EVID-", "evidence id", sequence)
    }

    pub fn sequence_number(&self) -> u32 {
        numeric_id_sequence(self.as_str(), "EVID-")
    }
}

impl PatchId {
    pub fn from_sequence(sequence: u32) -> Result<Self, TriadError> {
        numeric_id_from_sequence("PATCH-", "patch id", sequence)
    }

    pub fn sequence_number(&self) -> u32 {
        numeric_id_sequence(self.as_str(), "PATCH-")
    }
}

impl RunId {
    pub fn from_sequence(sequence: u32) -> Result<Self, TriadError> {
        numeric_id_from_sequence("RUN-", "run id", sequence)
    }

    pub fn sequence_number(&self) -> u32 {
        numeric_id_sequence(self.as_str(), "RUN-")
    }
}

fn is_valid_claim_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("REQ-") else {
        return false;
    };
    let Some((domain, seq)) = rest.rsplit_once('-') else {
        return false;
    };

    !domain.is_empty()
        && domain
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && seq.len() == 3
        && seq.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_valid_evidence_id(value: &str) -> bool {
    has_prefixed_numeric_suffix(value, "EVID-")
}

fn is_valid_patch_id(value: &str) -> bool {
    has_prefixed_numeric_suffix(value, "PATCH-")
}

fn is_valid_run_id(value: &str) -> bool {
    has_prefixed_numeric_suffix(value, "RUN-")
}

fn has_prefixed_numeric_suffix(value: &str, prefix: &str) -> bool {
    let Some(suffix) = value.strip_prefix(prefix) else {
        return false;
    };

    suffix.len() == 6 && suffix.bytes().all(|byte| byte.is_ascii_digit())
}

fn numeric_id_from_sequence<T>(prefix: &str, kind: &str, sequence: u32) -> Result<T, TriadError>
where
    T: TryFrom<String, Error = TriadError>,
{
    if !(1..=999_999).contains(&sequence) {
        return Err(TriadError::Parse(format!(
            "invalid {kind} sequence: {sequence}"
        )));
    }

    T::try_from(format!("{prefix}{sequence:06}"))
}

fn numeric_id_sequence(value: &str, prefix: &str) -> u32 {
    value
        .strip_prefix(prefix)
        .expect("numeric id prefix should match")
        .parse()
        .expect("numeric id suffix should parse")
}

#[cfg(test)]
mod tests {
    use serde_json::{from_str, to_string};

    use super::{ClaimId, EvidenceId, PatchId, RunId};

    #[test]
    fn ids_accept_valid_values() {
        assert_eq!(
            ClaimId::new("REQ-auth-login-001")
                .expect("valid claim id should parse")
                .as_str(),
            "REQ-auth-login-001"
        );
        assert_eq!(
            EvidenceId::new("EVID-000001")
                .expect("valid evidence id should parse")
                .as_str(),
            "EVID-000001"
        );
        assert_eq!(
            PatchId::new("PATCH-000001")
                .expect("valid patch id should parse")
                .as_str(),
            "PATCH-000001"
        );
        assert_eq!(
            RunId::new("RUN-000001")
                .expect("valid run id should parse")
                .as_str(),
            "RUN-000001"
        );
    }

    #[test]
    fn ids_reject_invalid_values() {
        assert!(ClaimId::new("REQ-auth-01").is_err());
        assert!(ClaimId::new("REQ-AUTH-001").is_err());
        assert!(EvidenceId::new("EVID-ABC123").is_err());
        assert!(PatchId::new("PATCH-12345").is_err());
        assert!(RunId::new("RUN-1234567").is_err());
    }

    #[test]
    fn ids_serde_roundtrip_preserves_validated_types() {
        let claim = ClaimId::new("REQ-auth-001").expect("valid claim id should parse");
        let evidence = EvidenceId::new("EVID-000001").expect("valid evidence id should parse");
        let patch = PatchId::new("PATCH-000001").expect("valid patch id should parse");
        let run = RunId::new("RUN-000001").expect("valid run id should parse");

        assert_eq!(
            from_str::<ClaimId>(&to_string(&claim).expect("claim id should serialize"))
                .expect("claim id should deserialize"),
            claim
        );
        assert_eq!(
            from_str::<EvidenceId>(&to_string(&evidence).expect("evidence id should serialize"))
                .expect("evidence id should deserialize"),
            evidence
        );
        assert_eq!(
            from_str::<PatchId>(&to_string(&patch).expect("patch id should serialize"))
                .expect("patch id should deserialize"),
            patch
        );
        assert_eq!(
            from_str::<RunId>(&to_string(&run).expect("run id should serialize"))
                .expect("run id should deserialize"),
            run
        );

        assert!(from_str::<ClaimId>("\"REQ-auth-01\"").is_err());
        assert!(from_str::<EvidenceId>("\"EVID-ABC123\"").is_err());
    }

    #[test]
    fn ids_support_owned_string_and_display_contracts() {
        let claim = ClaimId::try_from(String::from("REQ-auth-001"))
            .expect("owned string should convert into claim id");
        let evidence = EvidenceId::try_from(String::from("EVID-000001"))
            .expect("owned string should convert into evidence id");
        let patch = PatchId::try_from(String::from("PATCH-000001"))
            .expect("owned string should convert into patch id");
        let run = RunId::try_from(String::from("RUN-000001")).expect("owned string should convert");

        assert_eq!(claim.to_string(), "REQ-auth-001");
        assert_eq!(evidence.to_string(), "EVID-000001");
        assert_eq!(patch.to_string(), "PATCH-000001");
        assert_eq!(run.to_string(), "RUN-000001");

        assert_eq!(claim.into_inner(), "REQ-auth-001");
        assert_eq!(evidence.into_inner(), "EVID-000001");
        assert_eq!(patch.into_inner(), "PATCH-000001");
        assert_eq!(run.into_inner(), "RUN-000001");
    }

    #[test]
    fn numeric_ids_roundtrip_sequence_helpers() {
        let evidence = EvidenceId::from_sequence(42).expect("sequence should format");
        let patch = PatchId::from_sequence(42).expect("sequence should format");
        let run = RunId::from_sequence(42).expect("sequence should format");

        assert_eq!(evidence.as_str(), "EVID-000042");
        assert_eq!(patch.as_str(), "PATCH-000042");
        assert_eq!(run.as_str(), "RUN-000042");

        assert_eq!(evidence.sequence_number(), 42);
        assert_eq!(patch.sequence_number(), 42);
        assert_eq!(run.sequence_number(), 42);
    }

    #[test]
    fn numeric_ids_reject_out_of_range_sequences() {
        assert_eq!(
            EvidenceId::from_sequence(0)
                .expect_err("zero sequence should fail")
                .to_string(),
            "parse error: invalid evidence id sequence: 0"
        );
        assert_eq!(
            PatchId::from_sequence(1_000_000)
                .expect_err("overflow sequence should fail")
                .to_string(),
            "parse error: invalid patch id sequence: 1000000"
        );
        assert_eq!(
            RunId::from_sequence(0)
                .expect_err("zero sequence should fail")
                .to_string(),
            "parse error: invalid run id sequence: 0"
        );
    }
}
