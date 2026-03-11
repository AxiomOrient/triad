use crate::agent_runtime::{AdapterRunRequest, PromptAttachment, SessionConfig};
use camino::{Utf8Path, Utf8PathBuf};
use triad_core::{Claim, ClaimId, ReasoningLevel, RunClaimRequest, TriadError};

use crate::fs_support::write_json_file;
use crate::{
    LocalTriad, parsed_claim_by_id_or_issue, read_json_file, session_config_from_triad_in_root,
};

#[derive(Debug, Clone, PartialEq)]
pub struct WorkPromptEnvelope {
    pub claim: Claim,
    pub prompt: String,
    pub session_config: SessionConfig,
}

pub(crate) fn work_prompt_envelope(
    triad: &LocalTriad,
    claim_id: &ClaimId,
) -> Result<WorkPromptEnvelope, TriadError> {
    let claim = parsed_claim_by_id_or_issue(triad, claim_id)?;
    let session_config = work_session_config_for_claim(triad, claim_id, &triad.config.repo_root)?;

    Ok(WorkPromptEnvelope {
        claim,
        prompt: work_prompt_text(claim_id),
        session_config,
    })
}

fn claim_path_for_id(triad: &LocalTriad, claim_id: &ClaimId) -> Result<Utf8PathBuf, TriadError> {
    triad
        .claim_file_paths()?
        .into_iter()
        .find(|path| path.file_stem() == Some(claim_id.as_str()))
        .map(|path| {
            path.strip_prefix(&triad.config.repo_root)
                .map(Utf8PathBuf::from)
                .map_err(|_| {
                    TriadError::InvalidState(format!("claim path escaped repo root: {}", path))
                })
        })
        .transpose()?
        .ok_or_else(|| TriadError::InvalidState(format!("claim not found: {claim_id}")))
}

fn agent_run_schema_path(triad: &LocalTriad) -> Utf8PathBuf {
    triad.config.paths.schema_dir.join("agent.run.schema.json")
}

fn envelope_schema_path(triad: &LocalTriad) -> Utf8PathBuf {
    triad.config.paths.schema_dir.join("envelope.schema.json")
}

fn work_session_config_for_claim(
    triad: &LocalTriad,
    claim_id: &ClaimId,
    workspace_root: &Utf8Path,
) -> Result<crate::agent_runtime::SessionConfig, TriadError> {
    let claim_path = claim_path_for_id(triad, claim_id)?;
    let mut session_config = session_config_from_triad_in_root(&triad.config, workspace_root)?;
    session_config.attachments = vec![
        PromptAttachment::AtPath {
            path: "AGENTS.md".to_string(),
            placeholder: None,
        },
        PromptAttachment::AtPath {
            path: claim_path.to_string(),
            placeholder: None,
        },
    ];
    session_config.output_schema = Some(agent_run_output_schema(triad)?);
    Ok(session_config)
}

pub(crate) fn agent_run_output_schema(triad: &LocalTriad) -> Result<serde_json::Value, TriadError> {
    let run_schema = read_json_file(agent_run_schema_path(triad).as_std_path(), "output schema")
        .and_then(validate_agent_run_output_schema)?;
    let envelope_schema =
        read_json_file(envelope_schema_path(triad).as_std_path(), "envelope schema")
            .and_then(validate_agent_envelope_schema)?;
    flatten_agent_run_output_schema(&envelope_schema, &run_schema)
}

pub(crate) fn build_adapter_run_request(
    triad: &LocalTriad,
    req: &RunClaimRequest,
    workspace_root: &Utf8Path,
) -> Result<AdapterRunRequest, TriadError> {
    let _claim = parsed_claim_by_id_or_issue(triad, &req.claim_id)?;
    let schema = agent_run_output_schema(triad)?;
    let schema_path = workspace_root.join(".triad/runtime-agent.run.schema.json");
    write_json_file(schema_path.as_std_path(), &schema, "runtime output schema")?;

    Ok(AdapterRunRequest {
        backend: triad.config.agent.backend,
        claim_id: req.claim_id.clone(),
        repo_root: triad.config.repo_root.clone(),
        workspace_root: workspace_root.to_path_buf(),
        prompt_text: work_prompt_text(&req.claim_id),
        schema_path,
        model: resolved_run_model(triad, req),
        effort: resolved_run_effort(triad, req),
        timeout: std::time::Duration::from_secs(triad.config.agent.timeout_seconds),
        dry_run: req.dry_run,
        approval_policy: triad.config.agent.approval_policy.clone(),
        sandbox_policy: triad.config.agent.sandbox_policy.clone(),
        codex: triad.config.agent.codex.clone(),
        claude: triad.config.agent.claude.clone(),
        gemini: triad.config.agent.gemini.clone(),
    })
}

fn validate_agent_run_output_schema(
    schema: serde_json::Value,
) -> Result<serde_json::Value, TriadError> {
    let command_const = schema
        .pointer("/allOf/1/properties/command/const")
        .and_then(serde_json::Value::as_str);
    if command_const != Some("run") {
        return Err(TriadError::InvalidState(
            "agent.run schema must fix command to `run`".to_string(),
        ));
    }

    let data_required = schema
        .pointer("/allOf/1/properties/data/required")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            TriadError::InvalidState(
                "agent.run schema must declare required data fields".to_string(),
            )
        })?;
    for field in [
        "claim_id",
        "summary",
        "changed_paths",
        "suggested_test_selectors",
        "blocked_actions",
        "needs_patch",
    ] {
        if !data_required
            .iter()
            .any(|value| value.as_str() == Some(field))
        {
            return Err(TriadError::InvalidState(format!(
                "agent.run schema missing required data field: {field}"
            )));
        }
    }

    let data_additional_properties = schema
        .pointer("/allOf/1/properties/data/additionalProperties")
        .and_then(serde_json::Value::as_bool);
    if data_additional_properties != Some(false) {
        return Err(TriadError::InvalidState(
            "agent.run schema must disable additional properties in data".to_string(),
        ));
    }

    validate_schema_property_type(&schema, "claim_id", "string")?;
    validate_schema_property_type(&schema, "summary", "string")?;
    validate_schema_property_type(&schema, "needs_patch", "boolean")?;
    validate_schema_array_property_type(&schema, "changed_paths", "string")?;
    validate_schema_array_property_type(&schema, "suggested_test_selectors", "string")?;
    validate_schema_array_property_type(&schema, "blocked_actions", "string")?;

    Ok(schema)
}

fn validate_agent_envelope_schema(
    schema: serde_json::Value,
) -> Result<serde_json::Value, TriadError> {
    if schema.get("type").and_then(serde_json::Value::as_str) != Some("object") {
        return Err(TriadError::InvalidState(
            "envelope schema must declare a top-level object type".to_string(),
        ));
    }

    let required = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            TriadError::InvalidState("envelope schema must declare required fields".to_string())
        })?;
    for field in ["schema_version", "ok", "command", "data", "diagnostics"] {
        if !required.iter().any(|value| value.as_str() == Some(field)) {
            return Err(TriadError::InvalidState(format!(
                "envelope schema missing required field: {field}"
            )));
        }
    }

    if schema
        .get("additionalProperties")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        return Err(TriadError::InvalidState(
            "envelope schema must disable additional properties".to_string(),
        ));
    }

    if schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .is_none()
    {
        return Err(TriadError::InvalidState(
            "envelope schema must declare properties".to_string(),
        ));
    }

    Ok(schema)
}

fn flatten_agent_run_output_schema(
    envelope_schema: &serde_json::Value,
    run_schema: &serde_json::Value,
) -> Result<serde_json::Value, TriadError> {
    let envelope_properties = envelope_schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            TriadError::InvalidState("envelope schema must expose object properties".to_string())
        })?;
    let run_properties = run_schema
        .pointer("/allOf/1/properties")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            TriadError::InvalidState("agent.run schema must expose object properties".to_string())
        })?;

    let mut properties = envelope_properties.clone();
    for (key, value) in run_properties {
        properties.insert(key.clone(), value.clone());
    }

    let mut flattened = serde_json::json!({
        "$schema": envelope_schema["$schema"],
        "$id": run_schema["$id"],
        "title": run_schema["title"],
        "type": "object",
        "additionalProperties": envelope_schema["additionalProperties"],
        "required": envelope_schema["required"],
        "properties": properties,
    });
    strip_optional_object_properties(&mut flattened);
    Ok(flattened)
}

fn strip_optional_object_properties(schema: &mut serde_json::Value) {
    match schema {
        serde_json::Value::Object(map) => {
            let required_keys = map
                .get("required")
                .and_then(serde_json::Value::as_array)
                .map(|required| {
                    required
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_string)
                        .collect::<std::collections::BTreeSet<_>>()
                });
            if let (Some(required_keys), Some(properties)) = (
                required_keys,
                map.get_mut("properties")
                    .and_then(serde_json::Value::as_object_mut),
            ) {
                properties.retain(|key, _| required_keys.contains(key));
            }

            for value in map.values_mut() {
                strip_optional_object_properties(value);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_optional_object_properties(item);
            }
        }
        _ => {}
    }
}

fn validate_schema_property_type(
    schema: &serde_json::Value,
    field: &str,
    expected_type: &str,
) -> Result<(), TriadError> {
    let actual_type = schema
        .pointer(&format!("/allOf/1/properties/data/properties/{field}/type"))
        .and_then(serde_json::Value::as_str);
    if actual_type != Some(expected_type) {
        return Err(TriadError::InvalidState(format!(
            "agent.run schema field `{field}` must have type `{expected_type}`"
        )));
    }

    Ok(())
}

fn validate_schema_array_property_type(
    schema: &serde_json::Value,
    field: &str,
    expected_item_type: &str,
) -> Result<(), TriadError> {
    validate_schema_property_type(schema, field, "array")?;

    let actual_item_type = schema
        .pointer(&format!(
            "/allOf/1/properties/data/properties/{field}/items/type"
        ))
        .and_then(serde_json::Value::as_str);
    if actual_item_type != Some(expected_item_type) {
        return Err(TriadError::InvalidState(format!(
            "agent.run schema field `{field}` items must have type `{expected_item_type}`"
        )));
    }

    Ok(())
}

pub(crate) fn work_prompt_text(claim_id: &ClaimId) -> String {
    [
        "You are implementing exactly one triad claim.",
        "",
        &format!("Selected claim: {claim_id}"),
        "Use only the attached project rules and the attached selected claim as repository context.",
        "Do not infer or load unrelated claims or unrelated docs.",
        "",
        "Forbidden actions:",
        "- Write to spec/claims/** during work.",
        "- Run git commit or git push.",
        "- Remove files recursively outside an explicitly approved temporary workspace.",
        "- Modify files unrelated to the selected claim.",
        "",
        "Output requirements:",
        "- Return JSON only, matching the configured output schema.",
        "- changed_paths must list every modified repo file. Exclude ignored derived artifacts such as target/** and a newly generated Cargo.lock.",
        &format!("- Set claim_id to {claim_id}."),
    ]
    .join("\n")
}

fn resolved_run_model(triad: &LocalTriad, req: &RunClaimRequest) -> String {
    req.model
        .clone()
        .unwrap_or_else(|| triad.config.agent.model.clone())
}

fn resolved_run_effort(triad: &LocalTriad, req: &RunClaimRequest) -> String {
    req.effort
        .map(reasoning_level_wire_name)
        .unwrap_or_else(|| triad.config.agent.effort.clone())
}

fn reasoning_level_wire_name(level: ReasoningLevel) -> String {
    match level {
        ReasoningLevel::Low => "low".to_string(),
        ReasoningLevel::Medium => "medium".to_string(),
        ReasoningLevel::High => "high".to_string(),
    }
}
