use std::collections::HashMap;

use serde_json::Value;

use super::json::text;

/// When each quiet native owner began its unbroken hold on a queued job: the job's patience runs out only when one hold outlasts it, so back-to-back quiet runs each shorter than the patience keep the job queued instead of refusing it (north#436).
#[derive(Default)]
pub struct QuietHolds(HashMap<String, f64>);

impl QuietHolds {
    pub fn outlasts(&mut self, result: &Value, now_ms: f64, patience_seconds: Option<u64>) -> bool {
        let owners = if text(result, "reason") == Some("DEFER_NATIVE_QUIET") { held_by(result) } else { Vec::new() };
        self.0.retain(|owner, _| owners.contains(owner));
        for owner in owners {
            self.0.entry(owner).or_insert(now_ms);
        }
        patience_seconds.is_some_and(|patience| self.0.values().any(|since| (now_ms - since) / 1000.0 >= patience as f64))
    }
}

pub fn quiet_owners(result: &Value) -> Vec<String> {
    result["quietOwners"].as_array().map_or_else(Vec::new, |owners| owners.iter().map(|owner| owner.as_str().unwrap_or_default().to_owned()).collect())
}

fn held_by(result: &Value) -> Vec<String> {
    let owners = quiet_owners(result);
    if owners.is_empty() { vec![String::new()] } else { owners }
}
