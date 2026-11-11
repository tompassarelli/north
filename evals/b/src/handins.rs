use std::collections::BTreeMap;

use serde_json::{Value, json};

const HAND_IN_TOOL: &str = "mcp__north__hand-in";

#[derive(Clone, Copy)]
enum Outcome {
    BlindStaging,
    MaskedPipeline,
    OtherHook,
    FetchLock,
    RefusedBeforeQueue,
    RefusedByLander,
    Other,
}

const RULES: [(&[&str], Outcome); 6] = [
    (&["PreToolUse:Bash hook error", "blind staging"], Outcome::BlindStaging),
    (&["PreToolUse:Bash hook error", "masked-pipeline"], Outcome::MaskedPipeline),
    (&["PreToolUse:Bash hook error"], Outcome::OtherHook),
    (&["cannot lock ref"], Outcome::FetchLock),
    (&["cannot queue the lane: it conflicts"], Outcome::RefusedBeforeQueue),
    (&["lander did not land", "conflict"], Outcome::RefusedByLander),
];

fn outcome(text: &str) -> Outcome {
    RULES.iter().find(|(needles, _)| needles.iter().all(|n| text.contains(n))).map_or(Outcome::Other, |&(_, o)| o)
}

fn is_handin(part: &Value) -> bool {
    part["name"] == HAND_IN_TOOL || part["input"]["command"].as_str().is_some_and(|c| c.contains("safe-push --to main"))
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

#[derive(Default)]
struct Tally {
    handins: u64,
    by_tool: u64,
    reworks: u64,
    before_queue: u64,
    by_lander: u64,
    blind_staging: u64,
    masked_pipeline: u64,
    other_hook: u64,
    fetch_lock: u64,
    after_refusal: BTreeMap<String, bool>,
}

impl Tally {
    fn call(&mut self, part: &Value) {
        let rework = self.before_queue + self.by_lander > 0;
        self.handins += 1;
        self.by_tool += u64::from(part["name"] == HAND_IN_TOOL);
        self.reworks += u64::from(rework);
        self.after_refusal.insert(part["id"].as_str().unwrap_or_default().to_owned(), rework);
    }

    fn result(&mut self, part: &Value) {
        let Some(&rework) = self.after_refusal.get(part["tool_use_id"].as_str().unwrap_or_default()) else { return };
        let hook = match outcome(&text_of(&part["content"])) {
            Outcome::BlindStaging => &mut self.blind_staging,
            Outcome::MaskedPipeline => &mut self.masked_pipeline,
            Outcome::OtherHook => &mut self.other_hook,
            Outcome::FetchLock => return self.fetch_lock += 1,
            Outcome::RefusedBeforeQueue => return self.before_queue += 1,
            Outcome::RefusedByLander => return self.by_lander += 1,
            Outcome::Other => return,
        };
        *hook += 1;
        self.reworks -= u64::from(rework);
    }

    fn json(&self) -> Value {
        json!({"handins": self.handins, "handins_by_tool": self.by_tool, "reworks": self.reworks,
               "conflict_refusals_before_queue": self.before_queue, "conflict_refusals_by_lander": self.by_lander,
               "hook_blocks": {"blind_staging": self.blind_staging, "masked_pipeline": self.masked_pipeline, "other": self.other_hook},
               "fetch_lock_failures": self.fetch_lock})
    }
}

pub fn count(records: &[Value]) -> Value {
    let mut tally = Tally::default();
    let parts = records.iter().filter(|r| r["type"] == "assistant" || r["type"] == "user").flat_map(|r| r["message"]["content"].as_array().cloned().unwrap_or_default());
    for part in parts {
        match part["type"].as_str() {
            Some("tool_use") if is_handin(&part) => tally.call(&part),
            Some("tool_result") => tally.result(&part),
            _ => {}
        }
    }
    tally.json()
}
