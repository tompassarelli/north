//! Tom's drain switch: while on, the pooled reserve is skipped. It lives in `spend/drain.json` in the agents state directory.
use std::path::Path;

use serde_json::{Value, json};

const FILE: &str = "spend/drain.json";

fn read(agents: &Path) -> Value {
    std::fs::read(agents.join(FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}

pub fn on(agents: &Path, at: f64) -> bool {
    let state = read(agents);
    state["on"].as_bool().unwrap_or(false) && state["until"].as_f64().is_none_or(|until| at < until)
}

pub fn set(agents: &Path, on: bool, until: Option<f64>) -> std::io::Result<()> {
    let path = agents.join(FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json!({"on": on, "until": until}).to_string())
}

/// The first line of `north fleet usage`: which spend mode applies.
pub fn mode_line(agents: &Path, at: f64, reserve_pct: f64) -> String {
    if !on(agents, at) {
        return format!("Spend mode: default, {reserve_pct:.0}% of the combined week of all accounts stays in reserve (`north fleet drain on` spends it)");
    }
    match read(agents)["until"].as_f64() {
        Some(until) => format!("Spend mode: drain, no reserve, every account spent to its provider limit until unix time {until:.0} (`north fleet drain off` ends it)"),
        None => "Spend mode: drain, no reserve, every account spent to its provider limit (`north fleet drain off` ends it)".to_owned(),
    }
}
