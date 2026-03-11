use serde_json::json;
use triad_config::AgentBackend;
use triad_core::TriadError;

use super::{
    AdapterRunRequest, AgentRuntimeAdapter, PreparedProcessInvocation, ProcessCaptureMode,
    RawInvocationOutput,
};
use crate::agent_runtime::adapter::AdapterCompletion;

#[derive(Debug, Default)]
pub(crate) struct GeminiAdapter;

impl AgentRuntimeAdapter for GeminiAdapter {
    fn backend(&self) -> AgentBackend {
        AgentBackend::Gemini
    }

    fn prepare_invocation(
        &self,
        request: &AdapterRunRequest,
    ) -> Result<PreparedProcessInvocation, TriadError> {
        if request.backend != AgentBackend::Gemini {
            return Err(TriadError::config_field(
                "agent.backend",
                "gemini adapter requires backend = gemini",
            ));
        }

        if request.approval_policy != "never" {
            return Err(TriadError::config_field(
                "agent.approval_policy",
                "gemini one-shot backend only supports `never`",
            ));
        }

        if request.sandbox_policy == "danger-full-access" {
            return Err(TriadError::config_field(
                "agent.sandbox_policy",
                "gemini one-shot backend does not support danger-full-access",
            ));
        }

        let approval_mode = match request.sandbox_policy.as_str() {
            "workspace-write" => "yolo",
            "read-only" => "plan",
            other => {
                return Err(TriadError::config_field(
                    "agent.sandbox_policy",
                    &format!("unknown sandbox policy: {other}"),
                ));
            }
        };

        let args = vec![
            "-p".to_string(),
            request.prompt_text.clone(),
            "--output-format".to_string(),
            "json".to_string(),
            "--model".to_string(),
            request.model.clone(),
            "--approval-mode".to_string(),
            approval_mode.to_string(),
            "--sandbox".to_string(),
        ];

        Ok(PreparedProcessInvocation {
            program: "gemini".to_string(),
            args,
            env: Default::default(),
            cwd: request.workspace_root.clone(),
            stdin: None,
            timeout: request.timeout,
            capture_mode: ProcessCaptureMode::Stdout,
            model: request.model.clone(),
            effort: request.effort.clone(),
        })
    }

    fn complete(&self, output: RawInvocationOutput) -> Result<AdapterCompletion, TriadError> {
        if output.exit_code != 0 {
            let detail = if output.stderr.trim().is_empty() {
                output.stdout.trim()
            } else {
                output.stderr.trim()
            };
            return Err(TriadError::InvalidState(format!(
                "gemini -p failed with exit code {}: {}",
                output.exit_code, detail
            )));
        }

        extract_gemini_result(&output.stdout)
    }
}

fn extract_gemini_result(stdout: &str) -> Result<AdapterCompletion, TriadError> {
    let json: serde_json::Value = serde_json::from_str(stdout)
        .map_err(|err| TriadError::Parse(format!("failed to parse gemini json output: {err}")))?;

    let object = json.as_object().ok_or_else(|| {
        TriadError::InvalidState("gemini json output must be a single object".to_string())
    })?;

    if let Some(error) = object.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("gemini reported an error");
        return Err(TriadError::InvalidState(message.to_string()));
    }

    let response = object
        .get("response")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            TriadError::InvalidState(
                "gemini json output must include a string response".to_string(),
            )
        })?;

    let normalized = normalize_response_payload(response)?;

    Ok(AdapterCompletion {
        assistant_text: normalized,
    })
}

fn normalize_response_payload(response: &str) -> Result<String, TriadError> {
    let candidate = strip_markdown_code_fence(response).trim().to_string();
    let value: serde_json::Value = serde_json::from_str(&candidate).map_err(|err| {
        TriadError::InvalidState(format!(
            "gemini response must contain valid JSON for the triad run schema: {err}"
        ))
    })?;

    if !value.is_object() {
        return Err(TriadError::InvalidState(
            "gemini response must contain a JSON object for the triad run schema".to_string(),
        ));
    }

    normalize_gemini_run_payload(&value)
}

fn strip_markdown_code_fence(response: &str) -> &str {
    let trimmed = response.trim();
    if !trimmed.starts_with("```") {
        return trimmed;
    }

    let after_header = trimmed
        .find('\n')
        .map(|index| &trimmed[index + 1..])
        .unwrap_or(trimmed);
    if let Some(end) = after_header.rfind("```") {
        after_header[..end].trim()
    } else {
        trimmed
    }
}

fn normalize_gemini_run_payload(value: &serde_json::Value) -> Result<String, TriadError> {
    if value.get("schema_version").is_some()
        && value.get("command").is_some()
        && value.get("data").is_some()
    {
        return Ok(value.to_string());
    }

    let Some(claim_id) = value.get("claim_id").and_then(serde_json::Value::as_str) else {
        return Ok(value.to_string());
    };
    let changed_paths = string_array_field(value, "changed_paths")?;
    let summary = value
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(|status| format!("gemini reported {status}"))
        })
        .unwrap_or_else(|| "gemini completed work".to_string());
    let suggested_test_selectors =
        string_array_field_or_default(value, "suggested_test_selectors")?;
    let blocked_actions = string_array_field_or_default(value, "blocked_actions")?;
    let needs_patch = value
        .get("needs_patch")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let run_id = value
        .get("run_id")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    Ok(json!({
        "schema_version": 1,
        "ok": true,
        "command": "run",
        "data": {
            "run_id": run_id,
            "claim_id": claim_id,
            "summary": summary,
            "changed_paths": changed_paths,
            "suggested_test_selectors": suggested_test_selectors,
            "blocked_actions": blocked_actions,
            "needs_patch": needs_patch
        },
        "diagnostics": []
    })
    .to_string())
}

fn string_array_field(value: &serde_json::Value, field: &str) -> Result<Vec<String>, TriadError> {
    let array = value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            TriadError::InvalidState(format!(
                "gemini response must include {field} as a string array"
            ))
        })?;
    collect_string_array(array, field)
}

fn string_array_field_or_default(
    value: &serde_json::Value,
    field: &str,
) -> Result<Vec<String>, TriadError> {
    match value.get(field) {
        Some(serde_json::Value::Array(items)) => collect_string_array(items, field),
        Some(serde_json::Value::Null) | None => Ok(Vec::new()),
        Some(_) => Err(TriadError::InvalidState(format!(
            "gemini response must include {field} as a string array"
        ))),
    }
}

fn collect_string_array(
    items: &[serde_json::Value],
    field: &str,
) -> Result<Vec<String>, TriadError> {
    items
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                TriadError::InvalidState(format!(
                    "gemini response must include {field} as a string array"
                ))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use camino::Utf8PathBuf;
    use triad_config::AgentBackend;

    use super::GeminiAdapter;
    use crate::agent_runtime::{
        AdapterRunRequest, AgentRuntimeAdapter, ProcessCaptureMode, RawInvocationOutput,
    };

    #[test]
    fn gemini_adapter_prepares_print_invocation_with_json_output() {
        let adapter = GeminiAdapter;
        let request = test_request();

        let invocation = adapter
            .prepare_invocation(&request)
            .expect("gemini invocation should prepare");

        assert_eq!(invocation.program, "gemini");
        assert_eq!(
            invocation.cwd,
            Utf8PathBuf::from("/repo/.triad/tmp/workspaces/RUN-000001")
        );
        assert!(invocation.stdin.is_none());
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["-p", "prompt"])
        );
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--output-format", "json"])
        );
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--model", "gemini-2.5-pro"])
        );
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--approval-mode", "yolo"])
        );
        assert!(invocation.args.contains(&"--sandbox".to_string()));
        assert!(matches!(
            invocation.capture_mode,
            ProcessCaptureMode::Stdout
        ));
    }

    #[test]
    fn gemini_adapter_maps_read_only_sandbox_to_plan_mode() {
        let adapter = GeminiAdapter;
        let mut request = test_request();
        request.sandbox_policy = "read-only".to_string();

        let invocation = adapter
            .prepare_invocation(&request)
            .expect("gemini invocation should prepare");

        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--approval-mode", "plan"])
        );
        assert!(invocation.args.contains(&"--sandbox".to_string()));
    }

    #[test]
    fn gemini_adapter_rejects_non_never_approval_policy() {
        let adapter = GeminiAdapter;
        let mut request = test_request();
        request.approval_policy = "on-request".to_string();

        let error = adapter
            .prepare_invocation(&request)
            .expect_err("non-never approval must fail");

        assert_eq!(
            error.to_string(),
            "config error: invalid config agent.approval_policy: gemini one-shot backend only supports `never`"
        );
    }

    #[test]
    fn gemini_adapter_completes_wrapped_json_response() {
        let adapter = GeminiAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"response":"{\"ok\":true}","stats":{"models":{}}}"#.to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("structured response should normalize");

        assert_eq!(completion.assistant_text, "{\"ok\":true}");
    }

    #[test]
    fn gemini_adapter_wraps_partial_run_payload_into_envelope() {
        let adapter = GeminiAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"response":"{\"claim_id\":\"REQ-auth-001\",\"changed_paths\":[\"src/auth.rs\"],\"status\":\"done\"}","stats":{"models":{}}}"#.to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("partial run payload should normalize");

        let json: serde_json::Value =
            serde_json::from_str(&completion.assistant_text).expect("json should parse");
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["command"], "run");
        assert_eq!(json["data"]["claim_id"], "REQ-auth-001");
        assert_eq!(
            json["data"]["changed_paths"],
            serde_json::json!(["src/auth.rs"])
        );
        assert_eq!(json["data"]["summary"], "gemini reported done");
        assert_eq!(
            json["data"]["suggested_test_selectors"],
            serde_json::json!([])
        );
        assert_eq!(json["data"]["blocked_actions"], serde_json::json!([]));
        assert_eq!(json["data"]["needs_patch"], false);
    }

    #[test]
    fn gemini_adapter_completes_fenced_json_response() {
        let adapter = GeminiAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"response":"```json\n{\"ok\":true}\n```","stats":{"models":{}}}"#
                    .to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("fenced response should normalize");

        assert_eq!(completion.assistant_text, "{\"ok\":true}");
    }

    #[test]
    fn gemini_adapter_rejects_error_payload() {
        let adapter = GeminiAdapter;
        let error = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"error":{"message":"quota exceeded"}}"#.to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect_err("error payload should fail");

        assert_eq!(error.to_string(), "invalid state: quota exceeded");
    }

    fn test_request() -> AdapterRunRequest {
        AdapterRunRequest {
            backend: AgentBackend::Gemini,
            claim_id: triad_core::ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            repo_root: Utf8PathBuf::from("/repo"),
            workspace_root: Utf8PathBuf::from("/repo/.triad/tmp/workspaces/RUN-000001"),
            prompt_text: "prompt".to_string(),
            schema_path: Utf8PathBuf::from(
                "/repo/.triad/tmp/workspaces/RUN-000001/schemas/agent.run.schema.json",
            ),
            model: "gemini-2.5-pro".to_string(),
            effort: "medium".to_string(),
            timeout: Duration::from_secs(60),
            dry_run: false,
            approval_policy: "never".to_string(),
            sandbox_policy: "workspace-write".to_string(),
            codex: None,
            claude: None,
            gemini: Some(Default::default()),
        }
    }
}
