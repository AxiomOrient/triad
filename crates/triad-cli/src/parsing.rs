use anyhow::Result;
use triad_core::ClaimId;

pub(crate) fn parse_claim_id(raw: &str) -> Result<ClaimId> {
    ClaimId::new(raw).map_err(anyhow::Error::from)
}
