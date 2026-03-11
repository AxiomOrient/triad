use std::collections::BTreeMap;

use triad_config::AgentBackend;
use triad_core::TriadError;

use super::{
    AdapterRunRequest, AgentRuntimeAdapter, PreparedProcessInvocation, ProcessCaptureMode,
    RawInvocationOutput,
};
use crate::agent_runtime::adapter::AdapterCompletion;

#[derive(Debug, Default)]
pub(crate) struct CodexAdapter;

impl AgentRuntimeAdapter for CodexAdapter {
    fn backend(&self) -> AgentBackend {
        AgentBackend::Codex
    }

    fn prepare_invocation(
        &self,
        request: &AdapterRunRequest,
    ) -> Result<PreparedProcessInvocation, TriadError> {
        if request.backend != AgentBackend::Codex {
            return Err(TriadError::config_field(
                "agent.backend",
                "codex adapter requires backend = codex",
            ));
        }

        if request.approval_policy != "never" {
            return Err(TriadError::config_field(
                "agent.approval_policy",
                "codex one-shot backend only supports `never`",
            ));
        }

        if request.sandbox_policy == "danger-full-access" {
            return Err(TriadError::config_field(
                "agent.sandbox_policy",
                "codex one-shot backend does not support danger-full-access",
            ));
        }

        let codex = request.codex.clone().unwrap_or_default();
        if codex.local_provider.is_some() && !codex.use_oss {
            return Err(TriadError::config_field(
                "agent.codex.local_provider",
                "requires agent.codex.use_oss = true",
            ));
        }

        let output_path = request
            .workspace_root
            .join(".triad/codex-last-message.json");
        let mut args = vec![
            "exec".to_string(),
            "--skip-git-repo-check".to_string(),
            "--ephemeral".to_string(),
            "--output-schema".to_string(),
            request.schema_path.as_str().to_string(),
            "--output-last-message".to_string(),
            output_path.as_str().to_string(),
            "--model".to_string(),
            request.model.clone(),
            "--sandbox".to_string(),
            request.sandbox_policy.clone(),
        ];

        if let Some(profile) = codex.profile.as_ref() {
            args.push("--profile".to_string());
            args.push(profile.clone());
        }

        if codex.use_oss {
            args.push("--oss".to_string());
        }

        if let Some(local_provider) = codex.local_provider.as_ref() {
            args.push("--local-provider".to_string());
            args.push(local_provider.clone());
        }

        Ok(PreparedProcessInvocation {
            program: "codex".to_string(),
            args,
            env: BTreeMap::new(),
            cwd: request.workspace_root.clone(),
            stdin: Some(request.prompt_text.clone()),
            timeout: request.timeout,
            capture_mode: ProcessCaptureMode::OutputFile { path: output_path },
            model: request.model.clone(),
            effort: request.effort.clone(),
        })
    }

    fn complete(&self, output: RawInvocationOutput) -> Result<AdapterCompletion, TriadError> {
        if output.exit_code != 0 {
            return Err(TriadError::InvalidState(format!(
                "codex exec failed with exit code {}: {}",
                output.exit_code,
                output.stderr.trim()
            )));
        }

        Ok(AdapterCompletion {
            assistant_text: output.stdout,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use camino::Utf8PathBuf;
    use triad_config::{AgentBackend, CodexBackendConfig};

    use super::CodexAdapter;
    use crate::agent_runtime::{
        AdapterRunRequest, AgentRuntimeAdapter, ProcessCaptureMode, RawInvocationOutput,
    };

    #[test]
    fn codex_adapter_prepares_exec_invocation_with_schema_and_output_file() {
        let adapter = CodexAdapter;
        let request = test_request(CodexBackendConfig::default());

        let invocation = adapter
            .prepare_invocation(&request)
            .expect("codex invocation should prepare");

        assert_eq!(invocation.program, "codex");
        assert_eq!(
            invocation.cwd,
            Utf8PathBuf::from("/repo/.triad/tmp/workspaces/RUN-000001")
        );
        assert_eq!(invocation.stdin.as_deref(), Some("prompt"));
        assert!(invocation.args.windows(2).any(|window| window
            == [
                "--output-schema",
                "/repo/.triad/tmp/workspaces/RUN-000001/schemas/agent.run.schema.json"
            ]));
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--model", "gpt-5-codex"])
        );
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--sandbox", "workspace-write"])
        );
        assert!(matches!(
            invocation.capture_mode,
            ProcessCaptureMode::OutputFile { .. }
        ));
    }

    #[test]
    fn codex_adapter_adds_optional_oss_and_profile_flags() {
        let adapter = CodexAdapter;
        let request = test_request(CodexBackendConfig {
            use_oss: true,
            local_provider: Some("ollama".to_string()),
            profile: Some("local-oss".to_string()),
        });

        let invocation = adapter
            .prepare_invocation(&request)
            .expect("codex invocation should prepare");

        assert!(invocation.args.contains(&"--oss".to_string()));
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--local-provider", "ollama"])
        );
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--profile", "local-oss"])
        );
    }

    #[test]
    fn codex_adapter_rejects_non_never_approval_policy() {
        let adapter = CodexAdapter;
        let mut request = test_request(CodexBackendConfig::default());
        request.approval_policy = "on-request".to_string();

        let error = adapter
            .prepare_invocation(&request)
            .expect_err("non-never approval must fail");

        assert_eq!(
            error.to_string(),
            "config error: invalid config agent.approval_policy: codex one-shot backend only supports `never`"
        );
    }

    #[test]
    fn codex_adapter_rejects_local_provider_without_oss_mode() {
        let adapter = CodexAdapter;
        let request = test_request(CodexBackendConfig {
            use_oss: false,
            local_provider: Some("ollama".to_string()),
            profile: None,
        });

        let error = adapter
            .prepare_invocation(&request)
            .expect_err("local provider without oss should fail");

        assert_eq!(
            error.to_string(),
            "config error: invalid config agent.codex.local_provider: requires agent.codex.use_oss = true"
        );
    }

    #[test]
    fn codex_adapter_completes_output_from_last_message_file_capture() {
        let adapter = CodexAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: "{\"ok\":true}".to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("process output should normalize");

        assert_eq!(completion.assistant_text, "{\"ok\":true}");
    }

    fn test_request(codex: CodexBackendConfig) -> AdapterRunRequest {
        AdapterRunRequest {
            backend: AgentBackend::Codex,
            claim_id: triad_core::ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            repo_root: Utf8PathBuf::from("/repo"),
            workspace_root: Utf8PathBuf::from("/repo/.triad/tmp/workspaces/RUN-000001"),
            prompt_text: "prompt".to_string(),
            schema_path: Utf8PathBuf::from(
                "/repo/.triad/tmp/workspaces/RUN-000001/schemas/agent.run.schema.json",
            ),
            model: "gpt-5-codex".to_string(),
            effort: "medium".to_string(),
            timeout: Duration::from_secs(60),
            dry_run: false,
            approval_policy: "never".to_string(),
            sandbox_policy: "workspace-write".to_string(),
            codex: Some(codex),
            claude: None,
            gemini: None,
        }
    }
}
