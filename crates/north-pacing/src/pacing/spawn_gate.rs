use north_strategy_types::placement::SpawnCpu;
use serde_json::Value;
use std::sync::OnceLock;

pub struct SpawnGate {
    pub summary: fn() -> Value,
    /// The spawn gate's CPU reading for a local launch, None when unreadable.
    pub launch_capacity: fn() -> Option<SpawnCpu>,
    /// The refusal a local launch with this brief meets on that reading.
    pub launch_verdict: fn(&SpawnCpu, &str) -> Option<String>,
}

static GATE: OnceLock<SpawnGate> = OnceLock::new();

pub fn install(gate: SpawnGate) {
    let _ = GATE.set(gate);
}

pub fn summary() -> Value {
    GATE.get().map_or(Value::Null, |gate| (gate.summary)())
}

pub fn launch_capacity() -> Option<SpawnCpu> {
    GATE.get().and_then(|gate| (gate.launch_capacity)())
}

pub fn launch_verdict(cpu: &SpawnCpu, brief: &str) -> Option<String> {
    GATE.get().and_then(|gate| (gate.launch_verdict)(cpu, brief))
}
