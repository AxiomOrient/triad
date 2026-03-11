use std::fs;

use serde_json::json;
use triad_config::AgentBackend;
use triad_core::TriadError;

use super::{
    AdapterRunRequest, AgentRuntimeAdapter, PreparedProcessInvocation, ProcessCaptureMode,
    RawInvocationOutput,
};
use crate::agent_runtime::adapter::AdapterCompletion;

#[derive(Debug, Default)]
pub(crate) struct ClaudeAdapter;

impl AgentRuntimeAdapter for ClaudeAdapter {
    fn backend(&self) -> AgentBackend {
        AgentBackend::Claude
    }

    fn prepare_invocation(
        &self,
        request: &AdapterRunRequest,
    ) -> Result<PreparedProcessInvocation, TriadError> {
        if request.backend != AgentBackend::Claude {
            return Err(TriadError::config_field(
                "agent.backend",
                "claude adapter requires backend = claude",
            ));
        }

        if request.approval_policy != "never" {
            return Err(TriadError::config_field(
                "agent.approval_policy",
                "claude one-shot backend only supports `never`",
            ));
        }

        if request.sandbox_policy == "danger-full-access" {
            return Err(TriadError::config_field(
                "agent.sandbox_policy",
                "claude one-shot backend does not support danger-full-access",
            ));
        }

        let schema = fs::read_to_string(request.schema_path.as_std_path()).map_err(|err| {
            TriadError::Io(format!(
                "failed to read claude JSON schema {}: {err}",
                request.schema_path
            ))
        })?;

        let args = vec![
            "-p".to_string(),
            request.prompt_text.clone(),
            "--output-format".to_string(),
            "json".to_string(),
            "--json-schema".to_string(),
            schema,
            "--append-system-prompt".to_string(),
            "Return only the final JSON object matching the provided schema.".to_string(),
            "--model".to_string(),
            request.model.clone(),
            "--effort".to_string(),
            request.effort.clone(),
            "--no-session-persistence".to_string(),
            "--permission-mode".to_string(),
            request
                .claude
                .as_ref()
                .and_then(|config| config.permission_mode.as_ref())
                .cloned()
                .unwrap_or_else(|| "bypassPermissions".to_string()),
        ];

        Ok(PreparedProcessInvocation {
            program: "claude".to_string(),
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
            return Err(TriadError::InvalidState(format!(
                "claude -p failed with exit code {}: {}",
                output.exit_code,
                output.stderr.trim()
            )));
        }

        let completion = extract_claude_result(&output.stdout)?;

        Ok(AdapterCompletion {
            assistant_text: normalize_claude_run_payload(&completion.assistant_text)?,
        })
    }
}

fn extract_claude_result(stdout: &str) -> Result<AdapterCompletion, TriadError> {
    let json: serde_json::Value = serde_json::from_str(stdout)
        .map_err(|err| TriadError::Parse(format!("failed to parse claude json output: {err}")))?;

    let result = match &json {
        serde_json::Value::Object(object) => {
            if object
                .get("is_error")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                let message = object
                    .get("result")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("claude reported an error");
                return Err(TriadError::InvalidState(message.to_string()));
            }
            extract_claude_object_result(object)
        }
        serde_json::Value::Array(items) => items.iter().rev().find_map(|item| {
            item.get("type")
                .and_then(serde_json::Value::as_str)
                .filter(|value| *value == "result")
                .and_then(|_| item.get("result"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        }),
        _ => None,
    }
    .ok_or_else(|| {
        TriadError::InvalidState("claude json output must include a string result".to_string())
    })?;

    Ok(AdapterCompletion {
        assistant_text: result.to_string(),
    })
}

fn extract_claude_object_result(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    if let Some(result) = object
        .get("result")
        .and_then(serde_json::Value::as_str)
        .filter(|result| !result.trim().is_empty())
    {
        return normalize_claude_result_text(result);
    }

    object
        .get("structured_output")
        .filter(|value| value.is_object())
        .map(serde_json::Value::to_string)
}

fn normalize_claude_result_text(result: &str) -> Option<String> {
    let trimmed = result.trim();
    if let Some(json) = parse_json_object_string(trimmed) {
        return Some(json);
    }

    if let Some(fenced) = extract_fenced_block(trimmed) {
        if let Some(json) = parse_json_object_string(fenced) {
            return Some(json);
        }
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    parse_json_object_string(&trimmed[start..=end])
}

fn normalize_claude_run_payload(payload: &str) -> Result<String, TriadError> {
    let value: serde_json::Value = serde_json::from_str(payload).map_err(|err| {
        TriadError::InvalidState(format!(
            "claude normalized payload must contain valid JSON: {err}"
        ))
    })?;

    if value.get("schema_version").is_some()
        && value.get("command").is_some()
        && value.get("data").is_some()
    {
        return Ok(value.to_string());
    }

    let Some(claim_id) = value.get("claim_id").and_then(serde_json::Value::as_str) else {
        return Ok(value.to_string());
    };
    let summary = value
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            TriadError::InvalidState(
                "claude normalized payload must include summary or a full triad envelope"
                    .to_string(),
            )
        })?;
    let changed_paths = string_array_field(&value, "changed_paths")?;
    let suggested_test_selectors =
        string_array_field_or_default(&value, "suggested_test_selectors")?;
    let blocked_actions = string_array_field_or_default(&value, "blocked_actions")?;
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

fn parse_json_object_string(candidate: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(candidate).ok()?;
    value.is_object().then(|| value.to_string())
}

fn extract_fenced_block(text: &str) -> Option<&str> {
    let fence_start = text.find("```")?;
    let after_open = &text[fence_start + 3..];
    let after_language = if let Some(rest) = after_open.strip_prefix("json") {
        rest
    } else {
        after_open
    };
    let content = after_language.strip_prefix('\n').unwrap_or(after_language);
    let fence_end = content.find("```")?;
    Some(content[..fence_end].trim())
}

fn string_array_field(value: &serde_json::Value, field: &str) -> Result<Vec<String>, TriadError> {
    let array = value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            TriadError::InvalidState(format!(
                "claude normalized payload must include {field} as a string array"
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
            "claude normalized payload must include {field} as a string array"
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
                    "claude normalized payload must include {field} as a string array"
                ))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use camino::Utf8PathBuf;
    use triad_config::{AgentBackend, ClaudeBackendConfig};

    use super::ClaudeAdapter;
    use crate::agent_runtime::{
        AdapterRunRequest, AgentRuntimeAdapter, ProcessCaptureMode, RawInvocationOutput,
    };

    #[test]
    fn claude_adapter_prepares_print_invocation_with_schema_and_prompt_append() {
        let temp = temp_dir("claude-adapter-schema");
        let schema_path = temp.join("agent.run.schema.json");
        fs::write(&schema_path, r#"{"type":"object"}"#).expect("schema should be written");
        let adapter = ClaudeAdapter;
        let request = test_request(
            Utf8PathBuf::from_path_buf(schema_path.clone()).expect("path should be valid UTF-8"),
            Some(ClaudeBackendConfig {
                permission_mode: Some("acceptEdits".to_string()),
            }),
        );

        let invocation = adapter
            .prepare_invocation(&request)
            .expect("claude invocation should prepare");

        assert_eq!(invocation.program, "claude");
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
                .any(|window| window == ["--effort", "medium"])
        );
        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--permission-mode", "acceptEdits"])
        );
        assert!(
            invocation
                .args
                .contains(&"--append-system-prompt".to_string())
        );
        assert!(matches!(
            invocation.capture_mode,
            ProcessCaptureMode::Stdout
        ));
    }

    #[test]
    fn claude_adapter_defaults_permission_mode_to_bypass_permissions() {
        let temp = temp_dir("claude-adapter-default-permission");
        let schema_path = temp.join("agent.run.schema.json");
        fs::write(&schema_path, r#"{"type":"object"}"#).expect("schema should be written");
        let adapter = ClaudeAdapter;
        let request = test_request(
            Utf8PathBuf::from_path_buf(schema_path).expect("path should be valid UTF-8"),
            None,
        );

        let invocation = adapter
            .prepare_invocation(&request)
            .expect("claude invocation should prepare");

        assert!(
            invocation
                .args
                .windows(2)
                .any(|window| window == ["--permission-mode", "bypassPermissions"])
        );
    }

    #[test]
    fn claude_adapter_rejects_non_never_approval_policy() {
        let temp = temp_dir("claude-adapter-approval");
        let schema_path = temp.join("agent.run.schema.json");
        fs::write(&schema_path, r#"{"type":"object"}"#).expect("schema should be written");
        let adapter = ClaudeAdapter;
        let mut request = test_request(
            Utf8PathBuf::from_path_buf(schema_path).expect("path should be valid UTF-8"),
            None,
        );
        request.approval_policy = "on-request".to_string();

        let error = adapter
            .prepare_invocation(&request)
            .expect_err("non-never approval should fail");

        assert_eq!(
            error.to_string(),
            "config error: invalid config agent.approval_policy: claude one-shot backend only supports `never`"
        );
    }

    #[test]
    fn claude_adapter_completes_object_json_output() {
        let adapter = ClaudeAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"type":"result","subtype":"success","is_error":false,"result":"{\"ok\":true}"}"#.to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("structured output should normalize");

        assert_eq!(completion.assistant_text, "{\"ok\":true}");
    }

    #[test]
    fn claude_adapter_completes_structured_output_payload() {
        let adapter = ClaudeAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"ok":true}}"#.to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("structured_output payload should normalize");

        assert_eq!(completion.assistant_text, "{\"ok\":true}");
    }

    #[test]
    fn claude_adapter_completes_result_with_fenced_json_payload() {
        let adapter = ClaudeAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"type":"result","subtype":"success","is_error":false,"result":"Implementation complete.\n\n```json\n{\"claim_id\":\"REQ-auth-001\",\"summary\":\"done\",\"changed_paths\":[\"src/auth.rs\"]}\n```"}"#.to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("fenced result payload should normalize");

        let json: serde_json::Value =
            serde_json::from_str(&completion.assistant_text).expect("json should parse");
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["command"], "run");
        assert_eq!(json["data"]["claim_id"], "REQ-auth-001");
        assert_eq!(
            json["data"]["changed_paths"],
            serde_json::json!(["src/auth.rs"])
        );
        assert_eq!(
            json["data"]["suggested_test_selectors"],
            serde_json::json!([])
        );
        assert_eq!(json["data"]["blocked_actions"], serde_json::json!([]));
        assert_eq!(json["data"]["needs_patch"], false);
    }

    #[test]
    fn claude_adapter_completes_array_json_output() {
        let adapter = ClaudeAdapter;
        let completion = adapter
            .complete(RawInvocationOutput {
                stdout: r#"[{"type":"assistant","message":"ignored"},{"type":"result","result":"{\"ok\":true}"}]"#.to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect("array output should normalize");

        assert_eq!(completion.assistant_text, "{\"ok\":true}");
    }

    #[test]
    fn claude_adapter_rejects_error_result_payload() {
        let adapter = ClaudeAdapter;
        let error = adapter
            .complete(RawInvocationOutput {
                stdout: r#"{"type":"result","is_error":true,"result":"permission denied"}"#
                    .to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
            .expect_err("error payload should fail");

        assert_eq!(error.to_string(), "invalid state: permission denied");
    }

    fn test_request(
        schema_path: Utf8PathBuf,
        claude: Option<ClaudeBackendConfig>,
    ) -> AdapterRunRequest {
        AdapterRunRequest {
            backend: AgentBackend::Claude,
            claim_id: triad_core::ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            repo_root: Utf8PathBuf::from("/repo"),
            workspace_root: Utf8PathBuf::from("/repo/.triad/tmp/workspaces/RUN-000001"),
            prompt_text: "prompt".to_string(),
            schema_path,
            model: "claude-sonnet-4".to_string(),
            effort: "medium".to_string(),
            timeout: Duration::from_secs(60),
            dry_run: false,
            approval_policy: "never".to_string(),
            sandbox_policy: "workspace-write".to_string(),
            codex: None,
            claude,
            gemini: None,
        }
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "triad-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }
}
