use super::state::{prompts, py_str, truthy, with_state};
use super::{Ctx, asked_keyword, expected_profile, pattern, quoted_in};
use crate::hook::Verdict;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::LazyLock;

const CHECKLIST: &str = "End the plan's explanation with this checklist:\ngoal: \"<Tom's exact words from the request>\"\nprofile: <prototype|tooling|client>\ndone-when: <the pass/fail checks>\nextra checks: none | <check> catches <new failure>\nworkers: none | <n>, one per <issue or area>\neta: <minutes> min";
const CHECKLIST_KEYS: [&str; 6] = ["goal", "profile", "done-when", "extra checks", "workers", "eta"];

pattern!(ASKED, r#"(?i)asked:\s*"([^"]{4,})""#);
pattern!(LINES, r"(?im)^\s*(goal|profile|done-when|extra checks|workers|eta):\s*(.+)$");
pattern!(LEADING_NUMBER, r"^(\d+)");

static PLAN_PROCESS: LazyLock<Vec<(Regex, &str, &[&str])>> = LazyLock::new(|| {
    let rows: [(&str, &str, &[&str]); 9] = [
        (r"\b(independent|second|separate|fresh)[- ]?(review|reviewer|opinion|audit)\b", "an extra review", &["review", "audit"]),
        (r"\b(reviewer|verifier|auditor)s?\b", "a reviewer or verifier", &["review", "verif", "audit"]),
        (r"\battest(ation|ed|s)?\b|\bprovenance\b|\bsbom\b", "attestation or provenance", &["attest", "provenance", "sbom"]),
        (r"\bcanary\b|\bsoak\b", "a canary or soak", &["canary", "soak"]),
        (r"\b(re-?run|repeat)\b.{0,40}\b(passing|passed|for confidence)\b", "re-running a passing check", &["rerun", "re run", "again"]),
        (r"\bfor confidence\b|\bto be (safe|sure)\b|\bdouble[- ]check\b", "a confidence check", &["double check", "make sure"]),
        (r"\bcompat(ibility)? (shim|layer|bridge|path)\b|\bbackwards? compat\w*|\blegacy (client|caller|user)s?\b", "compatibility for users who don't exist", &["compat", "legacy"]),
        (r"\brollback (plan|path|copy|target)\b|\bmigration (plan|path)\b", "a rollback or migration plan", &["rollback", "migration"]),
        (r"\b(agents\.md|claude\.md|bootstrap|skill\.md)\b|\b(update|edit|amend|add|write)\b.{0,30}\b(skill|agent polic(y|ies))\b", "a policy or skill edit", &["agents md", "skill", "polic", "bootstrap", "harness", "hook"]),
    ];
    rows.into_iter().map(|(pattern, label, keywords)| (super::re(&format!("(?i){pattern}")), label, keywords)).collect()
});

pub(super) fn py_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn deny(reason: String) -> Option<Verdict> {
    Some(Verdict::Deny(reason))
}

pub(super) fn update_plan(ctx: &Ctx, event: &Value) -> Verdict {
    let tool_input = match event.get("tool_input") {
        Some(Value::String(text)) if !text.is_empty() => match serde_json::from_str::<Value>(text) {
            Ok(value) => value,
            Err(_) => return Verdict::Allow,
        },
        Some(value) if truthy(Some(value)) => value.clone(),
        _ => json!({}),
    };
    if !tool_input.is_object() {
        return Verdict::Allow;
    }
    let steps: Vec<&Value> = tool_input["plan"].as_array().map(|steps| steps.iter().filter(|s| s.is_object()).collect()).unwrap_or_default();
    let explanation = match tool_input.get("explanation") {
        Some(Value::String(text)) => text.as_str(),
        Some(value) if truthy(Some(value)) => return Verdict::Allow,
        _ => "",
    };
    let cwd = event["cwd"].as_str();
    with_state(ctx, &super::state::agent_key(event), |state| {
        check(ctx, state, &steps, explanation, cwd).unwrap_or(Verdict::Allow)
    })
}

fn process_step(steps: &[&Value], quoted: bool, recent: &[String]) -> Option<Verdict> {
    for (index, step) in steps.iter().enumerate() {
        if step["status"] == "completed" {
            continue;
        }
        let text = step.get("step").map_or_else(String::new, py_str);
        for (pattern, label, keywords) in PLAN_PROCESS.iter() {
            if !pattern.is_match(&text) || quoted {
                continue;
            }
            if !asked_keyword(keywords, recent) {
                return deny(format!(
                    "No. Step {} (\"{}\") adds {label}. Nobody asked for it, and it's the move that turns ten-minute tasks into five-hour ones. Delete the step and resubmit. If Tom literally asked for it, add asked: \"<his exact words>\" to the explanation.",
                    index + 1,
                    super::chars(&text, 80)
                ));
            }
        }
    }
    None
}

fn check(ctx: &Ctx, state: &mut serde_json::Map<String, Value>, steps: &[&Value], explanation: &str, cwd: Option<&str>) -> Option<Verdict> {
    let all = prompts(state, usize::MAX);
    let recent = prompts(state, 3);
    let quoted = ASKED.captures_iter(explanation).any(|m| quoted_in(&m[1], &all));
    if let Some(verdict) = process_step(steps, quoted, &recent) {
        return Some(verdict);
    }
    if steps.is_empty() || steps.iter().any(|s| s["status"] == "completed") {
        return None;
    }
    let lines: BTreeMap<String, String> = LINES.captures_iter(explanation).map(|m| (m[1].to_lowercase(), m[2].trim().to_owned())).collect();
    let missing: Vec<&str> = CHECKLIST_KEYS.into_iter().filter(|key| lines.get(*key).is_none_or(|v| v.is_empty())).collect();
    if !missing.is_empty() {
        return deny(format!("No. The plan is missing: {}. {CHECKLIST}", missing.join(", ")));
    }
    let goal = lines["goal"].trim().trim_matches(|c| matches!(c, '"' | '“' | '”'));
    if !all.is_empty() && !quoted_in(goal, &all) {
        return deny("No. The goal line must quote the request word for word, at least three words. Work nobody asked for is not the job. Re-read the request and plan only that.".into());
    }
    let cited = lines["profile"].split_whitespace().next().unwrap_or("").trim_matches(|c| matches!(c, '`' | '*' | '.' | ',')).to_lowercase();
    let expected = expected_profile(ctx, cwd);
    if cited != expected {
        return deny(format!(
            "No. The profile here is {}, not {} (bootstrap Profile table). Fix the line and size the plan to that profile.",
            py_repr(expected),
            py_repr(&cited)
        ));
    }
    let extra = lines["extra checks"].to_lowercase();
    if !extra.starts_with("none") && !extra.contains("catches") {
        return deny("No. Each extra check names the new failure it would catch: `extra checks: <check> catches <failure>`. Can't name one? Write `none` and drop the check.".into());
    }
    let minutes = LEADING_NUMBER.captures(&lines["eta"]).map(|m| m[1].parse::<u64>().unwrap_or(u64::MAX));
    let Some(minutes) = minutes.filter(|m| *m > 0) else {
        return deny("No. `eta:` must be a number of minutes, like `eta: 25 min`.".into());
    };
    state.insert("eta".into(), json!({"minutes": minutes, "set_at": ctx.now, "warned": false}));
    None
}
