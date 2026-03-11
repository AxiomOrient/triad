use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use triad_core::TriadError;

pub const CONFIG_FILE_NAME: &str = "triad.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriadConfig {
    pub version: u32,
    pub paths: PathConfig,
    pub agent: AgentConfig,
    pub verify: VerifyConfig,
    pub guardrails: GuardrailConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathConfig {
    pub claim_dir: Utf8PathBuf,
    pub docs_dir: Utf8PathBuf,
    pub state_dir: Utf8PathBuf,
    pub evidence_file: Utf8PathBuf,
    pub patch_dir: Utf8PathBuf,
    pub run_dir: Utf8PathBuf,
    pub schema_dir: Utf8PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub backend: AgentBackend,
    pub model: String,
    pub effort: String,
    pub approval_policy: String,
    pub sandbox_policy: String,
    pub timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<CodexBackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<ClaudeBackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini: Option<GeminiBackendConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentBackend {
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "gemini")]
    Gemini,
}

impl AgentBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodexBackendConfig {
    #[serde(default)]
    pub use_oss: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClaudeBackendConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GeminiBackendConfig {}

impl AgentConfig {
    pub fn validate(&self) -> Result<(), TriadError> {
        if self.timeout_seconds == 0 {
            return Err(TriadError::config_field(
                "agent.timeout_seconds",
                "must be > 0",
            ));
        }

        match self.backend {
            AgentBackend::Codex => {
                reject_backend_section("agent.claude", self.claude.is_some(), "claude")?;
                reject_backend_section("agent.gemini", self.gemini.is_some(), "gemini")?;
            }
            AgentBackend::Claude => {
                reject_backend_section("agent.codex", self.codex.is_some(), "codex")?;
                reject_backend_section("agent.gemini", self.gemini.is_some(), "gemini")?;
            }
            AgentBackend::Gemini => {
                reject_backend_section("agent.codex", self.codex.is_some(), "codex")?;
                reject_backend_section("agent.claude", self.claude.is_some(), "claude")?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyConfig {
    pub default_layers: Vec<String>,
    pub full_workspace_after_accept: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardrailConfig {
    pub forbid_direct_spec_edits: bool,
    pub forbid_git_commit: bool,
    pub forbid_git_push: bool,
    pub forbid_destructive_rm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalTriadConfig {
    pub repo_root: Utf8PathBuf,
    pub version: u32,
    pub paths: CanonicalPathConfig,
    pub agent: AgentConfig,
    pub verify: VerifyConfig,
    pub guardrails: GuardrailConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPathConfig {
    pub claim_dir: Utf8PathBuf,
    pub docs_dir: Utf8PathBuf,
    pub state_dir: Utf8PathBuf,
    pub evidence_file: Utf8PathBuf,
    pub patch_dir: Utf8PathBuf,
    pub run_dir: Utf8PathBuf,
    pub schema_dir: Utf8PathBuf,
}

impl TriadConfig {
    pub fn bootstrap_defaults() -> Self {
        Self {
            version: 1,
            paths: PathConfig {
                claim_dir: Utf8PathBuf::from("spec/claims"),
                docs_dir: Utf8PathBuf::from("docs"),
                state_dir: Utf8PathBuf::from(".triad"),
                evidence_file: Utf8PathBuf::from(".triad/evidence.ndjson"),
                patch_dir: Utf8PathBuf::from(".triad/patches"),
                run_dir: Utf8PathBuf::from(".triad/runs"),
                schema_dir: Utf8PathBuf::from("schemas"),
            },
            agent: AgentConfig {
                backend: AgentBackend::Codex,
                model: "gpt-5-codex".into(),
                effort: "medium".into(),
                approval_policy: "never".into(),
                sandbox_policy: "workspace-write".into(),
                timeout_seconds: 600,
                codex: Some(CodexBackendConfig::default()),
                claude: None,
                gemini: None,
            },
            verify: VerifyConfig {
                default_layers: vec!["unit".into(), "contract".into(), "integration".into()],
                full_workspace_after_accept: true,
            },
            guardrails: GuardrailConfig {
                forbid_direct_spec_edits: true,
                forbid_git_commit: true,
                forbid_git_push: true,
                forbid_destructive_rm: true,
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

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, TriadError> {
        let path = path.as_ref();
        let input = fs::read_to_string(path).map_err(|err| {
            TriadError::Io(format!(
                "failed to read triad config {}: {err}",
                path.display()
            ))
        })?;

        Self::from_toml_str(&input)
    }

    pub fn from_repo_root(repo_root: impl AsRef<Path>) -> Result<Self, TriadError> {
        Self::from_file(repo_root.as_ref().join(CONFIG_FILE_NAME))
    }

    pub fn canonicalize(
        self,
        repo_root: impl AsRef<Path>,
    ) -> Result<CanonicalTriadConfig, TriadError> {
        self.agent.validate()?;
        validate_non_empty_path("paths.claim_dir", &self.paths.claim_dir)?;
        validate_non_empty_path("paths.docs_dir", &self.paths.docs_dir)?;
        validate_non_empty_path("paths.state_dir", &self.paths.state_dir)?;
        validate_non_empty_path("paths.evidence_file", &self.paths.evidence_file)?;
        validate_non_empty_path("paths.patch_dir", &self.paths.patch_dir)?;
        validate_non_empty_path("paths.run_dir", &self.paths.run_dir)?;
        validate_non_empty_path("paths.schema_dir", &self.paths.schema_dir)?;

        let repo_root = repo_root.as_ref();
        let repo_root = normalize_utf8_path(repo_root)?;

        Ok(CanonicalTriadConfig {
            repo_root: repo_root.clone(),
            version: self.version,
            paths: CanonicalPathConfig {
                claim_dir: canonicalize_from_root(&repo_root, &self.paths.claim_dir)?,
                docs_dir: canonicalize_from_root(&repo_root, &self.paths.docs_dir)?,
                state_dir: canonicalize_from_root(&repo_root, &self.paths.state_dir)?,
                evidence_file: canonicalize_from_root(&repo_root, &self.paths.evidence_file)?,
                patch_dir: canonicalize_from_root(&repo_root, &self.paths.patch_dir)?,
                run_dir: canonicalize_from_root(&repo_root, &self.paths.run_dir)?,
                schema_dir: canonicalize_from_root(&repo_root, &self.paths.schema_dir)?,
            },
            agent: self.agent,
            verify: self.verify,
            guardrails: self.guardrails,
        })
    }
}

impl CanonicalTriadConfig {
    pub fn validate_for_init(self) -> Result<Self, TriadError> {
        validate_canonical_paths(&self)?;
        Ok(self)
    }

    pub fn validate(self) -> Result<Self, TriadError> {
        validate_canonical_paths(&self)?;
        validate_existing_dir("paths.docs_dir", &self.paths.docs_dir)?;
        validate_existing_dir("paths.schema_dir", &self.paths.schema_dir)?;
        Ok(self)
    }
}

fn validate_canonical_paths(config: &CanonicalTriadConfig) -> Result<(), TriadError> {
    validate_non_empty_path("paths.claim_dir", &config.paths.claim_dir)?;
    validate_non_empty_path("paths.docs_dir", &config.paths.docs_dir)?;
    validate_non_empty_path("paths.state_dir", &config.paths.state_dir)?;
    validate_non_empty_path("paths.evidence_file", &config.paths.evidence_file)?;
    validate_non_empty_path("paths.patch_dir", &config.paths.patch_dir)?;
    validate_non_empty_path("paths.run_dir", &config.paths.run_dir)?;
    validate_non_empty_path("paths.schema_dir", &config.paths.schema_dir)?;
    validate_child_path(
        "paths.evidence_file",
        &config.paths.state_dir,
        &config.paths.evidence_file,
    )?;
    validate_child_path(
        "paths.patch_dir",
        &config.paths.state_dir,
        &config.paths.patch_dir,
    )?;
    validate_child_path(
        "paths.run_dir",
        &config.paths.state_dir,
        &config.paths.run_dir,
    )?;
    Ok(())
}

pub fn discover_repo_root(start: impl AsRef<Path>) -> Result<PathBuf, TriadError> {
    let start = start.as_ref();
    let start_dir = if start.is_dir() {
        start
    } else {
        start.parent().ok_or_else(|| {
            TriadError::Config(format!(
                "failed to determine parent directory for {}",
                start.display()
            ))
        })?
    };

    for candidate in start_dir.ancestors() {
        if candidate.join(CONFIG_FILE_NAME).is_file() {
            return Ok(candidate.to_path_buf());
        }
    }

    Err(TriadError::Config(format!(
        "failed to find {CONFIG_FILE_NAME} from {}",
        start_dir.display()
    )))
}

fn canonicalize_from_root(
    repo_root: &Utf8Path,
    path: &Utf8Path,
) -> Result<Utf8PathBuf, TriadError> {
    if path.is_absolute() {
        return normalize_utf8_path(path);
    }

    normalize_utf8_path(repo_root.join(path))
}

fn normalize_utf8_path(path: impl AsRef<Path>) -> Result<Utf8PathBuf, TriadError> {
    let path = path.as_ref();
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| TriadError::Io(format!("failed to resolve current directory: {err}")))?
            .join(path)
    };

    let mut normalized = PathBuf::new();

    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    Utf8PathBuf::from_path_buf(normalized).map_err(|path_buf| {
        TriadError::Config(format!("path must be valid UTF-8: {}", path_buf.display()))
    })
}

fn validate_non_empty_path(field: &str, path: &Utf8Path) -> Result<(), TriadError> {
    if path.as_str().is_empty() {
        return Err(TriadError::config_field(field, "must not be empty"));
    }

    Ok(())
}

fn validate_existing_dir(field: &str, path: &Utf8Path) -> Result<(), TriadError> {
    if !path.exists() {
        return Err(TriadError::config_field(
            field,
            &format!("directory does not exist: {path}"),
        ));
    }

    if !path.is_dir() {
        return Err(TriadError::config_field(
            field,
            &format!("expected directory: {path}"),
        ));
    }

    Ok(())
}

fn validate_child_path(field: &str, parent: &Utf8Path, child: &Utf8Path) -> Result<(), TriadError> {
    if child.starts_with(parent) {
        return Ok(());
    }

    Err(TriadError::config_field(
        field,
        &format!("must stay under {parent}"),
    ))
}

fn reject_backend_section(
    field: &str,
    present: bool,
    expected_backend: &str,
) -> Result<(), TriadError> {
    if present {
        return Err(TriadError::config_field(
            field,
            &format!("is only allowed when agent.backend = \"{expected_backend}\""),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests;
