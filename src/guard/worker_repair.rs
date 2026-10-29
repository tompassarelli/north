use super::worker_tier::{brief_refusal, spawn_parts};
use crate::hook::HookInput;
use crate::platform::pyjson::Value;
use regex::Regex;

const ALIASES: &[(&str, &str, &str)] = &[
    ("Scope", "single-file", "one-file"),
    ("Scope", "one", "one-file"),
    ("Scope", "single-module", "module"),
    ("Scope", "single-repo", "cross-module"),
    ("Verify", "unit", "local-test"),
    ("Verify", "read-only", "none"),
    ("Verify", "report", "none"),
    ("Verify", "verdict", "none"),
    ("Verify", "reply", "none"),
    ("Verify", "review", "none"),
];

pub(super) const NOTE: &str = "worker-tier-guard: repaired the worker brief before spawning (run_in_background set to true; facet synonyms mapped to the ledger's facet values). Use the exact facet values next time.";

pub(super) fn repaired(input: &HookInput) -> Option<Value> {
    let (kind, prompt) = spawn_parts(input)?;
    let mut updated = input.tool_input.clone();
    let mut changed = false;
    if updated.get("run_in_background").is_some_and(|value| *value != Value::Bool(true)) {
        updated["run_in_background"] = Value::Bool(true);
        changed = true;
    }
    let mut fixed = prompt.to_owned();
    for (facet, from, to) in ALIASES {
        let pattern = Regex::new(&format!(r"(?mi)^([ \t]*{facet}:[ \t]*){from}\b")).unwrap();
        fixed = pattern.replace_all(&fixed, format!("${{1}}{to}")).into_owned();
    }
    if fixed != prompt {
        updated["prompt"] = fixed.as_str().into();
        changed = true;
    }
    (changed && brief_refusal(kind, false, false, &fixed).is_none()).then_some(updated)
}
