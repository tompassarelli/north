use super::briefs::{text, truthy};
use super::{World, state_dir};
use serde_json::Value;

/// The router's money refusal for the billed account, else the provider's, as `usage-gate.json` last
/// recorded it; a stale sample is used as it stands.
pub(super) fn money_refusal(world: &impl World, provider: &str) -> Result<Option<String>, String> {
    let Some(all) = std::fs::read_to_string(state_dir(world).join("agents/usage-gate.json")).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
        return Ok(None);
    };
    crate::platform::reader_stamp::touch(&state_dir(world), "agents/usage-gate.json");
    let verdict = &all[provider];
    let refusals = &verdict["refusals"];
    if truthy(refusals) && !refusals.is_object() {
        return Err("refusals is not a mapping".into());
    }
    let refusal = world.billed(provider).and_then(|account| refusals.get(account).cloned()).unwrap_or_else(|| verdict["refusal"].clone());
    Ok(truthy(&refusal).then(|| text(&refusal)))
}
