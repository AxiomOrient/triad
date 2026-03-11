use std::collections::BTreeMap;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use triad_core::{ClaimId, RunClaimReport, RunClaimRequest, RunId, TriadError};

use crate::LocalTriad;
use crate::agent_runtime::{PreparedProcessInvocation, RawInvocationOutput};
use crate::repo_support::sha256_prefixed_hex;

#[derive(Debug, Deserialize)]
struct AgentRunEnvelope {
    schema_version: u32,
    ok: bool,
    command: String,
    data: AgentRunData,
}

#[derive(Debug, Deserialize)]
struct AgentRunData {
    claim_id: ClaimId,
    summary: String,
    changed_paths: Vec<String>,
    suggested_test_selectors: Vec<String>,
    blocked_actions: Vec<String>,
    needs_patch: bool,
    run_id: Option<RunId>,
}

pub(crate) fn parse_run_claim_response(
    assistant_text: &str,
    expected_claim_id: &ClaimId,
    run_id: RunId,
) -> Result<RunClaimReport, TriadError> {
    let envelope: AgentRunEnvelope = serde_json::from_str(assistant_text)
        .map_err(|err| TriadError::Parse(format!("failed to parse work response JSON: {err}")))?;

    if envelope.schema_version != 1 {
        return Err(TriadError::InvalidState(format!(
            "work response must use schema_version 1, got {}",
            envelope.schema_version
        )));
    }
    if !envelope.ok {
        return Err(TriadError::InvalidState(
            "work response reported ok=false".to_string(),
        ));
    }
    if envelope.command != "run" {
        return Err(TriadError::InvalidState(format!(
            "work response command must be `run`, got `{}`",
            envelope.command
        )));
    }
    if &envelope.data.claim_id != expected_claim_id {
        return Err(TriadError::InvalidState(format!(
            "work response claim_id mismatch: expected {expected_claim_id}, got {}",
            envelope.data.claim_id
        )));
    }
    if let Some(reported_run_id) = envelope.data.run_id.as_ref() {
        if reported_run_id != &run_id {
            return Err(TriadError::InvalidState(format!(
                "work response run_id mismatch: expected {run_id}, got {reported_run_id}"
            )));
        }
    }

    Ok(RunClaimReport {
        run_id,
        claim_id: envelope.data.claim_id,
        summary: envelope.data.summary,
        changed_paths: envelope.data.changed_paths,
        suggested_test_selectors: envelope.data.suggested_test_selectors,
        blocked_actions: envelope.data.blocked_actions,
        needs_patch: envelope.data.needs_patch,
    })
}

pub(crate) fn prompt_fingerprint(prompt: &str) -> String {
    sha256_prefixed_hex(&Sha256::digest(prompt.as_bytes()))
}

pub(crate) fn work_runtime_metadata(
    triad: &LocalTriad,
    req: &RunClaimRequest,
    invocation: &PreparedProcessInvocation,
    output: &RawInvocationOutput,
) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "backend".to_string(),
        triad.config.agent.backend.as_str().to_string(),
    );
    metadata.insert("model".to_string(), invocation.model().to_string());
    metadata.insert("effort".to_string(), invocation.effort().to_string());
    metadata.insert(
        "approval_policy".to_string(),
        triad.config.agent.approval_policy.clone(),
    );
    metadata.insert(
        "sandbox_policy".to_string(),
        triad.config.agent.sandbox_policy.clone(),
    );
    metadata.insert("program".to_string(), invocation.program.clone());
    metadata.insert("exit_code".to_string(), output.exit_code.to_string());
    metadata.insert("dry_run".to_string(), req.dry_run.to_string());
    metadata
}
