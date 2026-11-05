use serde_json::Value;
use std::sync::OnceLock;

pub struct SpawnGate {
    pub summary: fn() -> Value,
    pub launch_refusal: fn(&str) -> Option<String>,
}

static GATE: OnceLock<SpawnGate> = OnceLock::new();

pub fn install(gate: SpawnGate) {
    let _ = GATE.set(gate);
}

pub fn summary() -> Value {
    GATE.get().map_or(Value::Null, |gate| (gate.summary)())
}

pub fn launch_refusal(brief: &str) -> Option<String> {
    GATE.get().and_then(|gate| (gate.launch_refusal)(brief))
}
