use crate::router::tier::Tier;
use serde_json::Value;
use std::sync::OnceLock;

pub type Pace = fn(&Value) -> Result<Value, String>;
pub type PickTier = fn(&str) -> Result<Tier, String>;
pub type RecordedDecisions = fn(&str, f64) -> Vec<String>;

#[derive(Clone, Copy)]
pub struct Host {
    pub pace: Pace,
    pub picked_tier: PickTier,
    pub recorded_decisions: RecordedDecisions,
}

static INSTALLED: OnceLock<Host> = OnceLock::new();

pub fn install(host: Host) {
    let _ = INSTALLED.set(host);
}

fn installed() -> Result<Host, String> {
    INSTALLED.get().copied().ok_or_else(|| "north hook: no guard host installed".to_owned())
}

pub fn pace(request: &Value) -> Result<Value, String> {
    installed().and_then(|host| (host.pace)(request))
}

pub fn picked_tier(brief: &str) -> Result<Tier, String> {
    installed().and_then(|host| (host.picked_tier)(brief))
}

pub fn recorded_decisions(paragraph: &str, now: f64) -> Vec<String> {
    installed().map_or_else(|_| Vec::new(), |host| (host.recorded_decisions)(paragraph, now))
}
