use std::io::Write;

use anyhow::Result;
use serde::Serialize;
use triad_core::{ClaimBundle, ClaimId, ClaimSummary};

pub(crate) const AGENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
struct AgentDiagnostic<'a> {
    level: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct AgentEnvelope<'a, T> {
    schema_version: u32,
    ok: bool,
    command: &'static str,
    data: &'a T,
    diagnostics: Vec<AgentDiagnostic<'static>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentClaimListData<'a> {
    claims: &'a [ClaimSummary],
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentClaimGetData<'a> {
    claim_id: &'a ClaimId,
    title: &'a str,
    statement: &'a str,
    examples: &'a [String],
    invariants: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<&'a str>,
    revision: u32,
}

pub(crate) fn write_agent_envelope<T: Serialize>(
    stdout: &mut impl Write,
    command: &'static str,
    data: &T,
) -> Result<()> {
    serde_json::to_writer(
        &mut *stdout,
        &AgentEnvelope {
            schema_version: AGENT_SCHEMA_VERSION,
            ok: true,
            command,
            data,
            diagnostics: Vec::new(),
        },
    )?;
    writeln!(&mut *stdout)?;
    Ok(())
}

pub(crate) fn agent_claim_list_data(claims: &[ClaimSummary]) -> AgentClaimListData<'_> {
    AgentClaimListData { claims }
}

pub(crate) fn agent_claim_get_data(bundle: &ClaimBundle) -> AgentClaimGetData<'_> {
    AgentClaimGetData {
        claim_id: &bundle.claim.id,
        title: &bundle.claim.title,
        statement: &bundle.claim.statement,
        examples: &bundle.claim.examples,
        invariants: &bundle.claim.invariants,
        notes: bundle.claim.notes.as_deref(),
        revision: bundle.claim.revision,
    }
}
