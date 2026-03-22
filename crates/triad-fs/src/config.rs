use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use triad_core::TriadError;

pub const CONFIG_FILE_NAME: &str = "triad.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriadConfig {
    pub version: u32,
    pub paths: PathConfig,
    pub snapshot: SnapshotConfig,
    pub verify: VerifyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathConfig {
    pub claim_dir: Utf8PathBuf,
    pub evidence_file: Utf8PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotConfig {
    pub include: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyConfig {
    pub commands: Vec<VerifyCommandConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VerifyCommandConfig {
    Legacy(String),
    Structured(StructuredVerifyCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredVerifyCommand {
    pub command: String,
    #[serde(default)]
    pub locator: Option<String>,
    #[serde(default)]
    pub artifacts: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalTriadConfig {
    pub repo_root: Utf8PathBuf,
    pub version: u32,
    pub paths: CanonicalPathConfig,
    pub snapshot: SnapshotConfig,
    pub verify: VerifyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPathConfig {
    pub claim_dir: Utf8PathBuf,
    pub evidence_file: Utf8PathBuf,
}

impl TriadConfig {
    pub fn bootstrap_defaults() -> Self {
        Self {
            version: 2,
            paths: PathConfig {
                claim_dir: Utf8PathBuf::from("spec/claims"),
                evidence_file: Utf8PathBuf::from(".triad/evidence.ndjson"),
            },
            snapshot: SnapshotConfig {
                include: vec![
                    "src/**".into(),
                    "tests/**".into(),
                    "crates/**".into(),
                    "triad.toml".into(),
                    "Cargo.toml".into(),
                    "Cargo.lock".into(),
                ],
            },
            verify: VerifyConfig {
                commands: vec![
                    VerifyCommandConfig::Legacy("cargo test --lib".into()),
                    VerifyCommandConfig::Legacy("cargo test --tests".into()),
                ],
            },
        }
    }

    pub fn bootstrap_toml() -> Result<String, TriadError> {
        toml::to_string_pretty(&Self::bootstrap_defaults()).map_err(|err| {
            TriadError::Serialization(format!("failed to serialize bootstrap triad config: {err}"))
        })
    }

    pub fn from_toml_str(input: &str) -> Result<Self, TriadError> {
        toml::from_str(input)
            .map_err(|err| TriadError::Parse(format!("failed to parse triad config: {err}")))
    }

    pub fn from_file(path: impl AsRef<Utf8Path>) -> Result<Self, TriadError> {
        let path = path.as_ref();
        let input = fs::read_to_string(path).map_err(|err| {
            TriadError::Io(format!("failed to read triad config {}: {err}", path))
        })?;

        Self::from_toml_str(&input)
    }

    pub fn canonicalize(
        self,
        repo_root: impl AsRef<Utf8Path>,
    ) -> Result<CanonicalTriadConfig, TriadError> {
        self.validate()?;
        self.normalize(repo_root.as_ref())
    }

    fn validate(&self) -> Result<(), TriadError> {
        if self.version != 2 {
            return Err(TriadError::config_field("version", "must be 2"));
        }

        validate_non_empty_path("paths.claim_dir", &self.paths.claim_dir)?;
        validate_non_empty_path("paths.evidence_file", &self.paths.evidence_file)?;
        validate_non_empty_strings("snapshot.include", &self.snapshot.include)?;
        validate_verify_commands(&self.verify.commands)
    }

    fn normalize(self, repo_root: &Utf8Path) -> Result<CanonicalTriadConfig, TriadError> {
        Ok(CanonicalTriadConfig {
            repo_root: repo_root.to_owned(),
            version: self.version,
            paths: CanonicalPathConfig {
                claim_dir: canonicalize_from_root(repo_root, &self.paths.claim_dir),
                evidence_file: canonicalize_from_root(repo_root, &self.paths.evidence_file),
            },
            snapshot: self.snapshot,
            verify: self.verify,
        })
    }
}

fn validate_non_empty_path(field: &str, path: &Utf8Path) -> Result<(), TriadError> {
    if path.as_str().trim().is_empty() {
        Err(TriadError::config_field(field, "must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_non_empty_strings(field: &str, values: &[String]) -> Result<(), TriadError> {
    if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
        Err(TriadError::config_field(
            field,
            "must contain non-empty values",
        ))
    } else {
        Ok(())
    }
}

fn validate_verify_commands(commands: &[VerifyCommandConfig]) -> Result<(), TriadError> {
    if commands.is_empty() {
        return Err(TriadError::config_field(
            "verify.commands",
            "must contain non-empty values",
        ));
    }

    for command in commands {
        match command {
            VerifyCommandConfig::Legacy(value) => {
                validate_non_empty_strings("verify.commands", std::slice::from_ref(value))?;
            }
            VerifyCommandConfig::Structured(value) => {
                if value.command.trim().is_empty() {
                    return Err(TriadError::config_field(
                        "verify.commands.command",
                        "must not be empty",
                    ));
                }
                if value
                    .locator
                    .as_deref()
                    .is_some_and(|locator| locator.trim().is_empty())
                {
                    return Err(TriadError::config_field(
                        "verify.commands.locator",
                        "must not be empty when present",
                    ));
                }
                if let Some(artifacts) = value.artifacts.as_deref() {
                    validate_non_empty_strings("verify.commands.artifacts", artifacts)?;
                }
            }
        }
    }

    Ok(())
}

impl VerifyCommandConfig {
    pub fn command(&self) -> &str {
        match self {
            VerifyCommandConfig::Legacy(value) => value,
            VerifyCommandConfig::Structured(value) => &value.command,
        }
    }

    pub fn locator(&self) -> Option<&str> {
        match self {
            VerifyCommandConfig::Legacy(_) => None,
            VerifyCommandConfig::Structured(value) => value.locator.as_deref(),
        }
    }

    pub fn artifacts(&self) -> Option<&[String]> {
        match self {
            VerifyCommandConfig::Legacy(_) => None,
            VerifyCommandConfig::Structured(value) => value.artifacts.as_deref(),
        }
    }
}

fn canonicalize_from_root(repo_root: &Utf8Path, path: &Utf8Path) -> Utf8PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        repo_root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;
    use triad_core::{TriadError, error::TriadErrorKind};

    use super::{CONFIG_FILE_NAME, TriadConfig, VerifyCommandConfig};

    fn temp_dir(label: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("triad-fs-{label}-{}-{unique}", process::id()));
        fs::create_dir_all(&path).expect("temp dir should create");
        Utf8PathBuf::from_path_buf(path).expect("temp dir path should be utf8")
    }

    fn config_toml(
        version: u32,
        commands_toml: &str,
        top_level_extra: &str,
        verify_extra: &str,
    ) -> String {
        let top_level_extra = if top_level_extra.is_empty() {
            String::new()
        } else {
            format!("{top_level_extra}\n")
        };
        let verify_extra = if verify_extra.is_empty() {
            String::new()
        } else {
            format!("{verify_extra}\n")
        };

        format!(
            "version = {version}\n{top_level_extra}\n[paths]\nclaim_dir = \"spec/claims\"\nevidence_file = \".triad/evidence.ndjson\"\n\n[snapshot]\ninclude = [\"crates/**\"]\n\n[verify]\ncommands = {commands_toml}\n{verify_extra}"
        )
    }

    #[test]
    fn bootstrap_toml_roundtrips_minimal_config() {
        let rendered = TriadConfig::bootstrap_toml().expect("bootstrap config should render");
        let parsed = TriadConfig::from_toml_str(&rendered).expect("bootstrap config should parse");

        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.paths.claim_dir, Utf8PathBuf::from("spec/claims"));
        assert_eq!(
            parsed.paths.evidence_file,
            Utf8PathBuf::from(".triad/evidence.ndjson")
        );
        assert!(
            parsed
                .snapshot
                .include
                .iter()
                .any(|path| path == "triad.toml")
        );
        assert_eq!(parsed.verify.commands.len(), 2);
        assert!(matches!(
            parsed.verify.commands[0],
            VerifyCommandConfig::Legacy(_)
        ));
    }

    #[test]
    fn canonicalize_resolves_relative_paths_from_repo_root() {
        let repo_root = temp_dir("canonicalize");
        let config = TriadConfig::bootstrap_defaults();

        let canonical = config
            .canonicalize(&repo_root)
            .expect("canonical config should build");

        assert_eq!(canonical.repo_root, repo_root);
        assert_eq!(
            canonical.paths.claim_dir,
            canonical.repo_root.join("spec/claims")
        );
        assert_eq!(
            canonical.paths.evidence_file,
            canonical.repo_root.join(".triad/evidence.ndjson")
        );
    }

    #[test]
    fn config_file_name_stays_stable() {
        assert_eq!(CONFIG_FILE_NAME, "triad.toml");
    }

    #[test]
    fn config_rejects_unknown_version() {
        let repo_root = temp_dir("config-version");
        let config = TriadConfig::from_toml_str(&config_toml(3, "[\"cargo test --lib\"]", "", ""))
            .expect("config should parse before canonical validation");

        let error = config
            .canonicalize(&repo_root)
            .expect_err("non-v2 config should be rejected");

        assert_eq!(
            error.to_string(),
            TriadError::config_field("version", "must be 2").to_string()
        );
    }

    #[test]
    fn config_rejects_unknown_top_level_field() {
        let error = TriadConfig::from_toml_str(&config_toml(
            2,
            "[\"cargo test --lib\"]",
            "extra = \"x\"",
            "",
        ))
        .expect_err("unknown top-level field should be rejected");

        assert!(error.to_string().contains("unknown field `extra`"));
    }

    #[test]
    fn config_rejects_unknown_nested_verify_field() {
        let error = TriadConfig::from_toml_str(&config_toml(
            2,
            "[\"cargo test --lib\"]",
            "",
            "extra = \"x\"",
        ))
        .expect_err("unknown nested verify field should be rejected");

        assert!(error.to_string().contains("unknown field `extra`"));
    }

    #[test]
    fn config_rejects_unknown_structured_verify_field() {
        let error = TriadConfig::from_toml_str(&config_toml(
            2,
            "[\n  { command = \"cargo test --lib\", typo = \"x\" }\n]",
            "",
            "",
        ))
        .expect_err("unknown structured verify field should be rejected");

        assert_eq!(error.kind(), TriadErrorKind::Parse);
        assert!(error.to_string().contains("failed to parse triad config"));
    }

    #[test]
    fn config_supports_mixed_legacy_and_structured_verify_commands() {
        let parsed = TriadConfig::from_toml_str(&config_toml(
            2,
            "[\n  \"cargo test --lib\",\n  { command = \"cargo test -- {claim_id}\", locator = \"cargo-test:{claim_id}\", artifacts = [\"crates/triad-core/**\"] }\n]",
            "",
            "",
        ))
        .expect("mixed config should parse");

        assert_eq!(parsed.verify.commands.len(), 2);
        assert_eq!(parsed.verify.commands[0].command(), "cargo test --lib");
        assert_eq!(
            parsed.verify.commands[1].locator(),
            Some("cargo-test:{claim_id}")
        );
        assert_eq!(
            parsed.verify.commands[1].artifacts(),
            Some(&["crates/triad-core/**".to_string()][..])
        );
    }
}
