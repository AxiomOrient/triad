use std::{collections::BTreeMap, time::Duration};

use camino::Utf8PathBuf;
use triad_config::{AgentBackend, ClaudeBackendConfig, CodexBackendConfig, GeminiBackendConfig};
use triad_core::{ClaimId, TriadError};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AdapterRunRequest {
    pub backend: AgentBackend,
    pub claim_id: ClaimId,
    pub repo_root: Utf8PathBuf,
    pub workspace_root: Utf8PathBuf,
    pub prompt_text: String,
    pub schema_path: Utf8PathBuf,
    pub model: String,
    pub effort: String,
    pub timeout: Duration,
    pub dry_run: bool,
    pub approval_policy: String,
    pub sandbox_policy: String,
    pub codex: Option<CodexBackendConfig>,
    pub claude: Option<ClaudeBackendConfig>,
    pub gemini: Option<GeminiBackendConfig>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedProcessInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Utf8PathBuf,
    pub stdin: Option<String>,
    pub timeout: Duration,
    pub capture_mode: ProcessCaptureMode,
    pub model: String,
    pub effort: String,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcessCaptureMode {
    Stdout,
    OutputFile { path: Utf8PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawInvocationOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdapterCompletion {
    pub assistant_text: String,
}

pub(crate) trait AgentRuntimeAdapter {
    fn backend(&self) -> AgentBackend;
    fn prepare_invocation(
        &self,
        request: &AdapterRunRequest,
    ) -> Result<PreparedProcessInvocation, TriadError>;
    fn complete(&self, output: RawInvocationOutput) -> Result<AdapterCompletion, TriadError>;
}

impl PreparedProcessInvocation {
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn effort(&self) -> &str {
        &self.effort
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};

    use camino::Utf8PathBuf;
    use triad_config::{AgentBackend, CodexBackendConfig};

    use super::{AdapterRunRequest, PreparedProcessInvocation, ProcessCaptureMode};

    #[test]
    fn adapter_contract_process_invocation_exposes_common_metadata() {
        let invocation = PreparedProcessInvocation {
            program: "claude".to_string(),
            args: vec!["-p".to_string(), "prompt".to_string()],
            env: BTreeMap::new(),
            cwd: Utf8PathBuf::from("/repo"),
            stdin: None,
            timeout: Duration::from_secs(30),
            capture_mode: ProcessCaptureMode::Stdout,
            model: "claude-sonnet".to_string(),
            effort: "medium".to_string(),
        };

        assert_eq!(invocation.program, "claude");
        assert_eq!(invocation.model(), "claude-sonnet");
        assert_eq!(invocation.effort(), "medium");
    }

    #[test]
    fn adapter_contract_request_preserves_backend_specific_codex_config() {
        let request = AdapterRunRequest {
            backend: AgentBackend::Codex,
            claim_id: triad_core::ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            repo_root: Utf8PathBuf::from("/repo"),
            workspace_root: Utf8PathBuf::from("/repo"),
            prompt_text: "prompt".to_string(),
            schema_path: Utf8PathBuf::from("/repo/schemas/agent.run.schema.json"),
            model: "gpt-5-codex".to_string(),
            effort: "medium".to_string(),
            timeout: Duration::from_secs(60),
            dry_run: false,
            approval_policy: "never".to_string(),
            sandbox_policy: "workspace-write".to_string(),
            codex: Some(CodexBackendConfig {
                use_oss: true,
                local_provider: Some("ollama".to_string()),
                profile: Some("local".to_string()),
            }),
            claude: None,
            gemini: None,
        };

        assert_eq!(
            request
                .codex
                .as_ref()
                .expect("codex config")
                .local_provider
                .as_deref(),
            Some("ollama")
        );
    }
}
