use triad_config::{AgentBackend, CanonicalTriadConfig};
use triad_core::{RunClaimRequest, TriadError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackendCapabilityProbe {
    pub backend: AgentBackend,
    pub supports_structured_json_output: bool,
    pub supports_native_output_schema: bool,
    pub supports_effort_override: bool,
    pub supports_permission_mode: bool,
}

pub(crate) fn probe_backend_capabilities(
    config: &CanonicalTriadConfig,
) -> Result<BackendCapabilityProbe, TriadError> {
    let probe = match config.agent.backend {
        AgentBackend::Codex => BackendCapabilityProbe {
            backend: AgentBackend::Codex,
            supports_structured_json_output: true,
            supports_native_output_schema: true,
            supports_effort_override: true,
            supports_permission_mode: false,
        },
        AgentBackend::Claude => BackendCapabilityProbe {
            backend: AgentBackend::Claude,
            supports_structured_json_output: true,
            supports_native_output_schema: true,
            supports_effort_override: true,
            supports_permission_mode: true,
        },
        AgentBackend::Gemini => BackendCapabilityProbe {
            backend: AgentBackend::Gemini,
            supports_structured_json_output: true,
            supports_native_output_schema: false,
            supports_effort_override: false,
            supports_permission_mode: false,
        },
    };

    Ok(probe)
}

pub(crate) fn validate_run_request_against_probe(
    config: &CanonicalTriadConfig,
    req: &RunClaimRequest,
    probe: &BackendCapabilityProbe,
) -> Result<(), TriadError> {
    if config.agent.backend != probe.backend {
        return Err(TriadError::InvalidState(format!(
            "backend capability probe mismatch: config={}, probe={}",
            config.agent.backend.as_str(),
            probe.backend.as_str()
        )));
    }

    if !probe.supports_structured_json_output {
        return Err(TriadError::config_field(
            "agent.backend",
            &format!(
                "{} backend does not support structured JSON output yet",
                probe.backend.as_str()
            ),
        ));
    }

    if req.effort.is_some() && !probe.supports_effort_override {
        return Err(TriadError::config_field(
            "agent.effort",
            &format!(
                "{} backend does not support effort override",
                probe.backend.as_str()
            ),
        ));
    }

    if config
        .agent
        .claude
        .as_ref()
        .and_then(|cfg| cfg.permission_mode.as_ref())
        .is_some()
        && !probe.supports_permission_mode
    {
        return Err(TriadError::config_field(
            "agent.claude.permission_mode",
            &format!(
                "{} backend does not support permission mode",
                probe.backend.as_str()
            ),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use triad_config::{
        AgentBackend, AgentConfig, CanonicalPathConfig, CanonicalTriadConfig, ClaudeBackendConfig,
        CodexBackendConfig, GeminiBackendConfig, GuardrailConfig, VerifyConfig,
    };
    use triad_core::{ClaimId, ReasoningLevel, RunClaimRequest};

    use super::{probe_backend_capabilities, validate_run_request_against_probe};

    #[test]
    fn backend_probe_reports_expected_capabilities_for_codex() {
        let config = test_config(AgentBackend::Codex);

        let probe = probe_backend_capabilities(&config).expect("probe should succeed");

        assert!(probe.supports_native_output_schema);
        assert!(probe.supports_structured_json_output);
        assert!(probe.supports_effort_override);
        assert!(!probe.supports_permission_mode);
    }

    #[test]
    fn backend_probe_reports_expected_capabilities_for_gemini() {
        let config = test_config(AgentBackend::Gemini);

        let probe = probe_backend_capabilities(&config).expect("probe should succeed");

        assert!(probe.supports_structured_json_output);
        assert!(!probe.supports_native_output_schema);
        assert!(!probe.supports_effort_override);
        assert!(!probe.supports_permission_mode);
    }

    #[test]
    fn backend_probe_rejects_effort_override_for_gemini() {
        let config = test_config(AgentBackend::Gemini);
        let probe = probe_backend_capabilities(&config).expect("probe should succeed");
        let request = RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: Some(ReasoningLevel::High),
        };

        let error = validate_run_request_against_probe(&config, &request, &probe)
            .expect_err("gemini effort override should be rejected");

        assert_eq!(
            error.to_string(),
            "config error: invalid config agent.effort: gemini backend does not support effort override"
        );
    }

    #[test]
    fn backend_probe_allows_claude_permission_mode() {
        let mut config = test_config(AgentBackend::Claude);
        config.agent.claude = Some(ClaudeBackendConfig {
            permission_mode: Some("acceptEdits".to_string()),
        });
        let probe = probe_backend_capabilities(&config).expect("probe should succeed");
        let request = RunClaimRequest {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            dry_run: false,
            model: None,
            effort: None,
        };

        validate_run_request_against_probe(&config, &request, &probe)
            .expect("claude permission mode should be allowed");
    }

    fn test_config(backend: AgentBackend) -> CanonicalTriadConfig {
        CanonicalTriadConfig {
            repo_root: Utf8PathBuf::from("/repo"),
            version: 1,
            paths: CanonicalPathConfig {
                claim_dir: Utf8PathBuf::from("/repo/spec/claims"),
                docs_dir: Utf8PathBuf::from("/repo/docs"),
                state_dir: Utf8PathBuf::from("/repo/.triad"),
                evidence_file: Utf8PathBuf::from("/repo/.triad/evidence.ndjson"),
                patch_dir: Utf8PathBuf::from("/repo/.triad/patches"),
                run_dir: Utf8PathBuf::from("/repo/.triad/runs"),
                schema_dir: Utf8PathBuf::from("/repo/schemas"),
            },
            agent: AgentConfig {
                backend,
                model: "test-model".to_string(),
                effort: "medium".to_string(),
                approval_policy: "never".to_string(),
                sandbox_policy: "workspace-write".to_string(),
                timeout_seconds: 60,
                codex: (backend == AgentBackend::Codex).then(CodexBackendConfig::default),
                claude: (backend == AgentBackend::Claude).then(ClaudeBackendConfig::default),
                gemini: (backend == AgentBackend::Gemini).then(GeminiBackendConfig::default),
            },
            verify: VerifyConfig {
                default_layers: vec!["unit".to_string()],
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
}
