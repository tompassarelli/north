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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::QuietHolds;

    const PATIENCE: Option<u64> = Some(1800);

    fn held(owner: &str) -> Value {
        json!({"decision": "DEFER", "reason": "DEFER_NATIVE_QUIET", "quietOwners": [owner]})
    }

    fn refused_at(holds: &[(&str, u64)]) -> Option<u64> {
        let mut quiet = QuietHolds::default();
        let mut second = 0;
        for (owner, length) in holds {
            for _ in 0..*length {
                if quiet.outlasts(&held(owner), second as f64 * 1000.0, PATIENCE) {
                    return Some(second);
                }
                second += 1;
            }
        }
        None
    }

    #[test]
    fn back_to_back_quiet_runs_each_shorter_than_the_patience_keep_the_job_queued_until_it_is_admitted() {
        assert_eq!(refused_at(&[("wisp-api-netcheck", 700), ("wisp-accept-pair-16", 700), ("perf-pacing-145", 700)]), None);
    }

    #[test]
    fn one_quiet_run_that_outlasts_the_patience_is_still_refused() {
        assert_eq!(refused_at(&[("wisp-api-netcheck", 300), ("perf-pacing-145", 2400)]), Some(300 + 1800));
    }

    #[test]
    fn a_quiet_owner_that_lets_go_starts_a_fresh_hold_when_it_returns() {
        let mut quiet = QuietHolds::default();
        assert!(!quiet.outlasts(&held("pool"), 0.0, PATIENCE));
        assert!(!quiet.outlasts(&json!({"decision": "DEFER", "reason": "DEFER_RUN_QUEUE"}), 1_000_000.0, PATIENCE));
        assert!(!quiet.outlasts(&held("pool"), 1_500_000.0, PATIENCE));
        assert!(!quiet.outlasts(&held("pool"), 1_900_000.0, PATIENCE));
        assert!(quiet.outlasts(&held("pool"), 3_300_000.0, PATIENCE));
    }
}
