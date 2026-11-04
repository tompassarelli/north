use crate::hook::{HookInput, Verdict};
use crate::ops::dispatch::Tier;
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

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*Item:[ \t]*\S").unwrap());
static TIER_REASON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[ \t]*Tier-reason:[ \t]*\S").unwrap());

fn routed_tier(brief: &str) -> Option<Tier> {
    crate::ops::dispatch::picked_tier(brief).ok()
}

fn asked_tier(kind: &str) -> Option<Tier> {
    match kind {
        "worker-haiku" => Some(Tier::Haiku),
        "worker-sonnet" => Some(Tier::Sonnet),
        "worker" | "worker-high" | "worker-xhigh" => Some(Tier::Opus),
        _ => None,
    }
}

/// Refuses a worker brief with no `Item:` line, and a Claude worker whose tier is not the one
/// `route()` picks for the brief unless the brief gives a `Tier-reason:`; escalations are exempt.
pub(super) fn routing_refusal(input: &HookInput, pick: &dyn Fn(&str) -> Option<Tier>) -> Option<String> {
    let (kind, prompt) = spawn_parts(input)?;
    if !ITEM.is_match(prompt) {
        return Some("Missing Item: name the issue or box the worker is for on an `Item:` line in the brief, so the ledger groups the run under it.".into());
    }
    let native = input.data["tool_name"].as_str().unwrap_or("").ends_with("spawn_agent");
    if native || FOLLOWS.is_match(prompt) || TIER_REASON.is_match(prompt) {
        return None;
    }
    let (asked, picked) = (asked_tier(kind)?, pick(prompt)?);
    (asked != picked).then(|| format!("{kind} is {}, but the ledger's routing picks {} for this brief; spawn that tier or add a `Tier-reason:` line saying why this box needs {kind}.", asked.name(), picked.name()))
}

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some(reason) = refusal(input).or_else(|| routing_refusal(input, &routed_tier)) else {
        return Verdict::Allow;
    };
    if crate::guard::activation::authoring_guards_off("worker-tier-guard") {
        return Verdict::Allow;
    }
    if input.data["tool_name"] == "Agent"
        && let Some(updated) = super::worker_repair::repaired(input)
    {
        return Verdict::Rewrite(updated, super::worker_repair::NOTE.into());
    }
    if let (Some(session), Some(tool_use_id)) = (
        input.data["session_id"].as_str().filter(|id| !id.is_empty()),
        input.data["tool_use_id"].as_str().filter(|id| !id.is_empty()),
    ) {
        super::spawn_state::release_pending_use(session, tool_use_id);
    }
    Verdict::Deny(reason)
}

pub(super) fn spawn_parts(input: &HookInput) -> Option<(&str, &str)> {
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
    kind.starts_with("worker").then_some((kind, prompt))
}

pub(super) fn refusal(input: &HookInput) -> Option<String> {
    let (kind, prompt) = spawn_parts(input)?;
    let native = input.data["tool_name"].as_str().unwrap_or("").ends_with("spawn_agent");
    let foreground = !native
        && input
            .tool_input
            .get("run_in_background")
            .is_some_and(|value| *value != crate::platform::pyjson::Value::Bool(true));
    brief_refusal(kind, native, foreground, prompt)
}

pub(super) fn brief_refusal(kind: &str, native: bool, foreground: bool, prompt: &str) -> Option<String> {
    let mut reasons = declared_refusals(foreground, prompt);
    reasons.extend(crate::router::brief::landing_refusal(prompt));
    if !native && kind == "worker-haiku" && DOCS.is_match(prompt) {
        reasons.push("Category: docs-policy goes to worker, not worker-haiku: Tom wants policy, skill and documentation prose written carefully by Opus. Haiku keeps only mechanical edits (renames, link fixes, box ticks) under Category: mechanical.".into());
    }
    if !FOLLOWS.is_match(prompt) {
        if !native && kind != "worker-haiku" && MECHANICAL.is_match(prompt) {
            reasons.push(format!("Category: mechanical goes to worker-haiku, not {kind} (ledger 28/31 mechanical). Escalate to worker only after a Haiku failure, and say so with a `Follows:` line naming the failed run."));
        }
        if matches!(kind, "worker-high" | "worker-xhigh") {
            reasons.push(format!("{kind} is escalation-only: assign worker first, and escalate only after its reasoning failure, saying so with a `Follows:` line naming the failed run."));
        }
    }
    (!reasons.is_empty()).then(|| reasons.join("\n"))
}

fn declared_refusals(foreground: bool, prompt: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    if foreground {
        reasons.push("Claude worker Agents require run_in_background: true so the background Agent result records the original prompt with the actual child identity at creation.".to_owned());
    }
    let undeclared = FIELDS.iter().filter(|field| !field.declared.is_match(prompt)).map(|field| field.name);
    reasons.extend(undeclared.map(|name| format!("Missing {name}: declare Category and Spec/Scope/Surface/Verify in the worker brief before spawning.")));
    reasons.extend(unknown_facets(prompt));
    reasons.extend(missing_payback(prompt));
    reasons
}

fn unknown_facets(prompt: &str) -> Vec<String> {
    FIELDS.iter().filter(|field| !field.allowed.is_empty()).filter_map(|field| {
        field.value.captures_iter(prompt).find_map(|found| {
            let value = &found[1];
            (!field.allowed.contains(&value.to_lowercase().as_str())).then(|| {
                let shown = if value.is_empty() { "(empty)" } else { value };
                format!("{}: {shown} is not a facet value; use one of {}, so north worker-ledger can group runs by it.", field.name, field.allowed.join(" | "))
            })
        })
    }).collect()
}

fn missing_payback(prompt: &str) -> Option<String> {
    let infra = INFRA.captures(prompt)?;
    (!PAYBACK.is_match(prompt)).then(|| format!("Category: {} needs a Payback: line priced with the planning skill: `Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days` or, for enabling work, `Payback: enables <what>; cost <M> min`.", &infra[1]))
}
