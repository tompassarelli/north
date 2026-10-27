use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::sync::LazyLock;

const FACETS: [(&str, &[&str]); 4] = [
    ("Spec", &["exact", "measured", "judged"]),
    ("Scope", &["one-file", "module", "cross-module", "cross-repo"]),
    ("Surface", &["ts", "rust", "lua", "nix", "shell", "workflow", "docs", "assets"]),
    ("Verify", &["none", "local-test", "farm", "native", "visual"]),
];
const DECLARED: [&str; 5] = ["Category", "Spec", "Scope", "Surface", "Verify"];
const VALUE: &str = r"[ \t]*([^\s.,;`'<>|)]*)";

struct Field {
    name: &'static str,
    declared: Regex,
    value: Regex,
    allowed: &'static [&'static str],
}

static FIELDS: LazyLock<Vec<Field>> = LazyLock::new(|| {
    DECLARED
        .iter()
        .map(|name| Field {
            name,
            declared: Regex::new(&format!(r"(?m)^[ \t]*{name}:[ \t]*[^\s.,;`'<>|)]")).unwrap(),
            value: Regex::new(&format!(r"(?m)^[ \t]*{name}:{VALUE}")).unwrap(),
            allowed: FACETS
                .iter()
                .find(|(facet, _)| facet == name)
                .map_or(&[], |(_, values)| values),
        })
        .collect()
});
static INFRA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?mi)^[ \t]*Category:\W*(tooling|infra|ci|harness)\b").unwrap());
static PAYBACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?mi)\bPayback:[ \t]*(saves[ \t]+\d[^\n]*\bpayback[ \t]+[\d.]+[ \t]*days?\b|\S[^\n]*\bcost[ \t]+~?\d)",
    )
    .unwrap()
});
static DOCS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?mi)^[ \t]*Category:\W*docs-policy\b").unwrap());
static FOLLOWS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Follows:\s*\S").unwrap());
static MECHANICAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?mi)^[ \t]*Category:\W*mechanical\b").unwrap());

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some(reason) = refusal(input) else {
        return Verdict::Allow;
    };
    if crate::hook::authoring_guards_off("worker-tier-guard") {
        return Verdict::Allow;
    }
    if let (Some(session), Some(tool_use_id)) = (
        input.data["session_id"].as_str().filter(|id| !id.is_empty()),
        input.data["tool_use_id"].as_str().filter(|id| !id.is_empty()),
    ) {
        super::spawn_state::release_pending_use(session, tool_use_id);
    }
    Verdict::Deny(reason)
}

pub(super) fn refusal(input: &HookInput) -> Option<String> {
    let tool = input.data["tool_name"].as_str().unwrap_or("");
    let native = tool.ends_with("spawn_agent");
    if tool != "Agent" && !native {
        return None;
    }
    let (kind, prompt) = if native {
        ("worker", input.tool_input["message"].as_str()?)
    } else {
        (
            input.tool_input["subagent_type"].as_str()?,
            input.tool_input["prompt"].as_str()?,
        )
    };
    if !kind.starts_with("worker") {
        return None;
    }
    let foreground = !native
        && input
            .tool_input
            .get("run_in_background")
            .is_some_and(|value| *value != crate::pyjson::Value::Bool(true));
    brief_refusal(kind, native, foreground, prompt)
}

fn brief_refusal(kind: &str, native: bool, foreground: bool, prompt: &str) -> Option<String> {
    if let Some(reason) = declared_refusal(foreground, prompt) {
        return Some(reason);
    }
    if !native && kind == "worker-haiku" && DOCS.is_match(prompt) {
        return Some("Category: docs-policy goes to worker, not worker-haiku: Tom wants policy, skill and documentation prose written carefully by Opus. Haiku keeps only mechanical edits (renames, link fixes, box ticks) under Category: mechanical.".into());
    }
    if FOLLOWS.is_match(prompt) {
        return None;
    }
    if !native && kind != "worker-haiku" && MECHANICAL.is_match(prompt) {
        return Some(format!("Category: mechanical goes to worker-haiku, not {kind} (ledger 28/31 mechanical). Escalate to worker only after a Haiku failure, and say so with a `Follows:` line naming the failed run."));
    }
    matches!(kind, "worker-high" | "worker-xhigh").then(|| format!("{kind} is escalation-only: assign worker first, and escalate only after its reasoning failure, saying so with a `Follows:` line naming the failed run."))
}

fn declared_refusal(foreground: bool, prompt: &str) -> Option<String> {
    let incomplete = if foreground {
        Some("Claude worker Agents require run_in_background: true so the background Agent result records the original prompt with the actual child identity at creation.".to_owned())
    } else {
        FIELDS.iter().find(|field| !field.declared.is_match(prompt)).map(|field| format!("Missing {}: declare Category and Spec/Scope/Surface/Verify in the worker brief before spawning.", field.name))
    };
    unknown_facet(prompt, incomplete.is_some()).or(incomplete).or_else(|| missing_payback(prompt))
}

fn unknown_facet(prompt: &str, already_refused: bool) -> Option<String> {
    // The retired shell guard stopped after the Spec facet once a refusal was set; decisions stay equal to it.
    let checked = if already_refused { 1 } else { FACETS.len() };
    FIELDS.iter().filter(|field| !field.allowed.is_empty()).take(checked).find_map(|field| {
        field.value.captures_iter(prompt).find_map(|found| {
            let value = &found[1];
            (!field.allowed.contains(&value.to_lowercase().as_str())).then(|| {
                let shown = if value.is_empty() { "(empty)" } else { value };
                format!("{}: {shown} is not a facet value; use one of {}, so worker-ledger can group runs by it.", field.name, field.allowed.join(" | "))
            })
        })
    })
}

fn missing_payback(prompt: &str) -> Option<String> {
    let infra = INFRA.captures(prompt)?;
    (!PAYBACK.is_match(prompt)).then(|| format!("Category: {} needs a Payback: line priced with the planning skill: `Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days` or, for enabling work, `Payback: enables <what>; cost <M> min`.", &infra[1]))
}
