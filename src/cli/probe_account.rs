use crate::error::{NorthError, NorthResult};
use crate::fleet::model::ProviderName;
use crate::fleet::runtime::Invocation;
use crate::ops::pacing::placement::gate;
use crate::providers::claude;

use super::fleet;

/// How the contract probe starts the runtime: as the Claude account the placement gate rates with the
/// most headroom.
pub(super) fn invocation() -> NorthResult<Invocation> {
    let roster = fleet::roster()?;
    let label = gate::roomiest_here(&gate::live_gate(), &roster)
        .ok_or_else(|| NorthError::Refused("no Claude account has headroom for the probe; retrying at the next pass".into()))?;
    let account = roster.account(&ProviderName::new("claude"), &label)?;
    Ok(claude::invocation(&account))
}
