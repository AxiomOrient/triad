use anyhow::{Result, anyhow};
use triad_core::{ClaimId, PatchId, TriadApi, VerifyRequest};

use crate::CliRuntime;
use crate::cli::AcceptArgs;

pub(crate) fn resolve_verify_request<R: CliRuntime>(
    runtime: &R,
    claim_id: Option<String>,
    with_probe: bool,
    full_workspace: bool,
) -> Result<VerifyRequest> {
    runtime
        .default_verify_request(
            resolve_claim_id(runtime, claim_id)?,
            with_probe,
            full_workspace,
        )
        .map_err(anyhow::Error::from)
}

pub(crate) fn resolve_claim_id<R: TriadApi>(
    runtime: &R,
    claim_id: Option<String>,
) -> Result<ClaimId> {
    match claim_id {
        Some(raw) => parse_claim_id(&raw),
        None => Ok(runtime.next_claim()?.claim_id),
    }
}

pub(crate) fn parse_optional_claim_id(raw: Option<&str>) -> Result<Option<ClaimId>> {
    raw.map(parse_claim_id).transpose()
}

pub(crate) fn parse_claim_id(raw: &str) -> Result<ClaimId> {
    ClaimId::new(raw).map_err(anyhow::Error::from)
}

pub(crate) fn parse_patch_id(raw: &str) -> Result<PatchId> {
    PatchId::new(raw).map_err(anyhow::Error::from)
}

pub(crate) fn resolve_accept_patch_id<R: CliRuntime>(
    runtime: &R,
    args: AcceptArgs,
) -> Result<PatchId> {
    match (args.patch_id.as_deref(), args.latest) {
        (Some(raw), false) => parse_patch_id(raw),
        (None, true) => runtime
            .latest_pending_patch_id()?
            .ok_or_else(|| anyhow!("no pending patch exists for --latest")),
        (Some(_), true) => Err(anyhow!("patch id and --latest are mutually exclusive")),
        (None, false) => Err(anyhow!("patch id is required unless --latest is set")),
    }
}
