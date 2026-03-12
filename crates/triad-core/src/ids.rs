use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::TriadError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClaimId(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceId(String);

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

impl EvidenceId {
    pub fn from_sequence(sequence: u32) -> Result<Self, TriadError> {
        numeric_id_from_sequence("EVID-", "evidence id", sequence)
    }

    pub fn sequence_number(&self) -> u32 {
        numeric_id_sequence(self.as_str(), "EVID-")
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

    use super::{ClaimId, EvidenceId};

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
    }

    #[test]
    fn ids_reject_invalid_values() {
        assert!(ClaimId::new("REQ-auth-01").is_err());
        assert!(ClaimId::new("REQ-AUTH-001").is_err());
        assert!(EvidenceId::new("EVID-ABC123").is_err());
    }

    #[test]
    fn ids_serde_roundtrip_preserves_validated_types() {
        let claim = ClaimId::new("REQ-auth-001").expect("valid claim id should parse");
        let evidence = EvidenceId::new("EVID-000001").expect("valid evidence id should parse");

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

        assert!(from_str::<ClaimId>("\"REQ-auth-01\"").is_err());
        assert!(from_str::<EvidenceId>("\"EVID-ABC123\"").is_err());
    }

    #[test]
    fn evidence_id_supports_sequence_helpers() {
        let evidence = EvidenceId::from_sequence(42).expect("sequence should format");

        assert_eq!(evidence.as_str(), "EVID-000042");
        assert_eq!(evidence.sequence_number(), 42);
    }
}
