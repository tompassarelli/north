#![cfg(test)]

use super::worker_tier::{brief_refusal, is_native, spawn_parts};
use crate::hook::HookInput;
use serde_json::{Value, json};

const FACETS: &str = "\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test";
const PAYBACK: &str = "Payback: saves 4 min/change (Tom 0.3, agent 1), 20 changes/day, cost 45 min, payback 0.5 days";

pub(super) fn refusal(input: &HookInput) -> Option<String> {
    let (kind, prompt) = spawn_parts(input)?;
    let native = is_native(input);
    let foreground = !native && input.tool_input.get("run_in_background").is_some_and(|value| *value != crate::platform::pyjson::Value::Bool(true));
    brief_refusal(kind, native, foreground, prompt)
}

fn input(agent: &str, background: Value, prompt: &str) -> HookInput {
    let payload = if agent == "codex" {
        json!({"tool_name": "spawn_agent", "tool_input": {"message": prompt, "task_name": "worker"}})
    } else {
        let mut tool_input = json!({"description": "d", "prompt": prompt});
        if !agent.is_empty() {
            tool_input["subagent_type"] = json!(agent);
        }
        if background != json!("missing") {
            tool_input["run_in_background"] = background;
        }
        json!({"tool_name": "Agent", "tool_input": tool_input})
    };
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

fn verdict(agent: &str, prompt: &str) -> Option<String> {
    refusal(&input(agent, json!(true), prompt))
}

fn feature() -> String {
    format!("Item: add a parser.\nCategory: feature{FACETS}")
}

fn mechanical() -> String {
    format!("Item: rename a field.\nCategory: mechanical\nDone when: check passes.{FACETS}")
}

fn escalation() -> String {
    format!("Follows: worker-haiku run a1b2 failed on the check.\nCategory: mechanical{FACETS}")
}

fn tiers_follow_the_category_and_escalation_rules() {
    let docs = format!("Item: fix a skill line. ETA 5 min.\nCategory: docs-policy{FACETS}");
    for (denied, agent, prompt) in [
        (true, "worker", mechanical()),
        (true, "worker-high", mechanical()),
        (false, "worker-haiku", mechanical()),
        (false, "worker", docs.clone()),
        (true, "worker-haiku", docs.clone()),
        (true, "worker-high", docs),
        (false, "worker", escalation()),
        (true, "worker-high", feature()),
        (true, "worker-xhigh", feature()),
        (false, "worker-high", format!("Follows: worker run c3 wrong cause twice.\n{}", feature())),
        (false, "worker-xhigh", format!("Follows: worker-high run d4 failed.\n{}", feature())),
        (false, "codex", feature()),
        (false, "codex", mechanical()),
        (false, "worker", feature()),
        (false, "Explore", mechanical()),
        (false, "", mechanical()),
        (false, "fork", mechanical()),
    ] {
        assert_eq!(verdict(agent, &prompt).is_some(), denied, "{agent}: {prompt}");
    }
}

fn a_claude_worker_must_run_in_the_background_even_when_it_follows_a_failure() {
    for background in [json!(false), json!("true"), json!(null)] {
        let reason = refusal(&input("worker", background.clone(), &escalation()));
        assert!(reason.is_some_and(|text| text.contains("run_in_background: true")), "{background}");
    }
    assert!(refusal(&input("worker", json!("missing"), &escalation())).is_none());
    assert!(refusal(&input("codex", json!(false), &escalation())).is_none());
    assert!(refusal(&input("Explore", json!(false), &escalation())).is_none());
}

fn every_missing_or_unknown_facet_is_refused_at_any_worker_tier() {
    for field in ["Category", "Spec", "Scope", "Surface", "Verify"] {
        let incomplete: String = feature().lines().filter(|line| !line.starts_with(&format!("{field}:"))).collect::<Vec<_>>().join("\n");
        for agent in ["worker", "codex"] {
            assert!(verdict(agent, &incomplete).is_some_and(|text| text.starts_with(&format!("Missing {field}:"))), "{agent} {field}");
        }
        assert!(verdict("worker-high", &format!("Follows: worker failed.\n{incomplete}")).is_some(), "escalated {field}");
    }
    for (prompt, text) in [
        ("Category: tooling\nSpec: measured\nScope: whole-repo\nSurface: shell\nVerify: local-test", "Scope: whole-repo is not a facet value"),
        ("Follows: worker run c3 failed.\nCategory: tooling\nSpec: measured\nScope: module\nSurface: shell\nVerify: eyeball", "Verify: eyeball is not a facet value"),
        ("Category: feature\nSpec: measured\nScope: section\nSurface: shell\nVerify: local-test", "Scope: section is not a facet value"),
    ] {
        assert!(verdict("worker", prompt).or(verdict("worker-high", prompt)).is_some_and(|reason| reason.contains(text)), "{prompt}");
    }
    assert!(verdict("worker", "Category: feature\nSpec: measured\nItem: tidy. Scope: module.\nSurface: shell\nVerify: local-test").is_some());
}

fn facet_words_inside_prose_do_not_count_as_declarations() {
    for item in ["Item: tidy the guard message Missing Scope: declare.", "Item: check the Scope: section of the brief."] {
        assert!(verdict("worker", &format!("{item}\nCategory: feature{FACETS}")).is_none(), "{item}");
    }
    let prose = format!("Item: reword the note that says Category: mechanical goes to Haiku.\nCategory: tooling\nPayback: enables x; cost 5 min{FACETS}");
    assert!(verdict("worker", &prose).is_none());
}

fn infrastructure_categories_need_a_priced_payback_line() {
    for category in ["tooling", "infra", "ci", "harness"] {
        let unpriced = verdict("worker", &format!("Category: {category}{FACETS}"));
        assert!(unpriced.is_some_and(|text| text.contains("planning skill") && text.contains(category)), "{category}");
        assert!(verdict("codex", &format!("Category: {category}{FACETS}")).is_some(), "native {category}");
        assert!(verdict("worker", &format!("Category: {category}\n{PAYBACK}{FACETS}")).is_none(), "{category} saving");
    }
    for payback in ["enables iteration ledger; cost 30 min", "stops unpriced boxes; cost ~45 agent-min"] {
        assert!(verdict("worker", &format!("Category: tooling\nPayback: {payback}{FACETS}")).is_none(), "{payback}");
    }
    assert!(verdict("worker", &format!("Category: tooling\nPayback: worth it{FACETS}")).is_some());
    assert!(verdict("worker-high", &format!("Follows: worker run c3 failed.\nCategory: tooling{FACETS}")).is_some());
    let known = "Category: tooling\nSpec: measured\nScope: one-file\nSurface: rust\nVerify: local-test\nPayback: enables x; cost 5 min";
    assert!(verdict("worker", known).is_none());
}

fn other_tools_and_malformed_inputs_pass() {
    for payload in [
        r#"{"tool_input": {"subagent_type": "worker"}"#,
        r#"{"tool_name":"Agent","tool_input":false}"#,
        r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
    ] {
        if let Some(parsed) = HookInput::parse(payload.as_bytes()) {
            assert!(refusal(&parsed).is_none(), "{payload}");
        }
    }
}

#[test]
fn decisions_match_the_recorded_guard_on_257_briefs_and_name_its_reason_first_among_all() {
    let recorded: Vec<Value> = serde_json::from_str(include_str!("fixtures/worker_tier.json")).unwrap();
    assert_eq!(recorded.len(), 257);
    for case in recorded {
        let agent = case["agent"].as_str().unwrap();
        if case.get("decision").is_some() {
            assert_eq!(decided(agent, case["background"].clone(), case["prompt"].as_str().unwrap()), case["decision"], "{case}");
            continue;
        }
        let brief = input(agent, case["background"].clone(), case["prompt"].as_str().unwrap());
        let now = refusal(&brief);
        match case["reason"].as_str() {
            Some(reason) => assert!(now.as_deref().is_some_and(|all| all.contains(reason)), "{case}\n{now:?}"),
            None => assert_eq!(now, None, "{case}"),
        }
    }
}

#[test]
fn worker_tier_decisions_hold() {
    tiers_follow_the_category_and_escalation_rules();
    a_claude_worker_must_run_in_the_background_even_when_it_follows_a_failure();
    every_missing_or_unknown_facet_is_refused_at_any_worker_tier();
    facet_words_inside_prose_do_not_count_as_declarations();
    infrastructure_categories_need_a_priced_payback_line();
    other_tools_and_malformed_inputs_pass();
}

#[test]
fn a_native_worker_brief_must_land_before_its_capture() {
    let native = "Item: fix the arrow streak.\nCategory: bug-known-cause\nSpec: exact\nScope: module\nSurface: ts\nVerify: native";
    assert!(verdict("worker", native).is_some_and(|why| why.contains("Lands-before: native")));
    assert_eq!(verdict("worker", &format!("{native}\nLands-before: native")), None);
}

fn decided(agent: &str, background: Value, prompt: &str) -> Value {
    use super::worker_tier::{Decision, decision};
    match decision(&input(agent, background, prompt), &|_| None) {
        None => json!({"reason": null}),
        Some(Decision::Deny(reason)) => json!({"reason": reason}),
        Some(Decision::Rewrite(updated, context)) => {
            let updated: Value = serde_json::from_str(&crate::platform::pyjson::dumps(&updated)).unwrap();
            json!({"reason": null, "updatedInput": updated, "additionalContext": context})
        }
    }
}

#[test]
fn a_foreground_spawn_or_facet_synonym_is_repaired_when_nothing_else_is_wrong() {
    let research = || feature().replace("Category: feature", "Category: research");
    let updated = decided("worker", json!(false), &research());
    assert_eq!(updated["updatedInput"]["run_in_background"], json!(true));
    assert_eq!(updated["updatedInput"]["prompt"].as_str(), Some(research().as_str()));

    let synonyms = research()
        .replace("Scope: module", "Scope: single-repo")
        .replace("Verify: local-test", "Verify: read-only");
    let updated = decided("worker", json!(true), &synonyms);
    let prompt = updated["updatedInput"]["prompt"].as_str().unwrap();
    assert!(prompt.contains("Scope: cross-module") && prompt.contains("Verify: none"), "{prompt}");
    assert_eq!(updated["updatedInput"]["run_in_background"], json!(true));
    assert_eq!(decided("worker", json!(true), &research().replace("Scope: module", "Scope: one-file")), json!({"reason": null}));

    for (agent, background, prompt) in [
        ("worker", json!(false), research().replace("Spec: measured", "Spec: Peon")),
        ("worker", json!(true), research().replace("Scope: module", "Scope: read-only")),
        ("worker", json!(false), "Item: x".to_owned()),
        ("worker-high", json!(false), research()),
        ("worker", json!(false), feature()),
    ] {
        assert!(decided(agent, background, &prompt)["reason"].is_string(), "{prompt}");
    }
}

#[test]
fn a_brief_breaking_several_rules_gets_every_refusal_in_one_message() {
    let prompt = "Item: rename a field.\nCategory: mechanical\nSpec: measured\nScope: whole-repo\nVerify: eyeball";
    let reason = refusal(&input("worker-high", json!(false), prompt)).unwrap();
    for named in [
        "run_in_background: true",
        "Missing Surface:",
        "Scope: whole-repo is not a facet value",
        "Verify: eyeball is not a facet value",
        "Category: mechanical goes to worker-haiku",
        "worker-high is escalation-only",
    ] {
        assert!(reason.contains(named), "{named} missing from:\n{reason}");
    }
    let infra = format!("Category: tooling{FACETS}\nLands-before: later");
    let reason = verdict("worker-haiku", &infra).unwrap();
    assert!(reason.contains("needs a Payback: line") && reason.contains("Lands-before: later is not a landing order"), "{reason}");
}

mod routing {
    use super::{FACETS, input};
    use crate::guard::worker_tier::{is_native, routing_refusal, spawn_parts};
    use crate::router::tier::Tier;
    use serde_json::json;

    fn tooling() -> String {
        format!("Item: add a flag.\nCategory: tooling\nPayback: enables x; cost 5 min{FACETS}")
    }

    fn verdict(agent: &str, prompt: &str, pick: Option<Tier>) -> Option<String> {
        {
        let brief = input(agent, json!(true), prompt);
        let (kind, prompt) = spawn_parts(&brief)?;
        routing_refusal(kind, is_native(&brief), prompt, &|_| pick)
    }
    }

    #[test]
    fn an_agent_whose_tier_differs_from_the_routed_pick_is_refused_unless_the_brief_gives_a_tier_reason() {
        let refused = verdict("worker", &tooling(), Some(Tier::Sonnet)).unwrap();
        assert!(refused.contains("picks sonnet-high") && refused.contains("Tier-reason:"), "{refused}");
        assert!(verdict("worker-haiku", &tooling(), Some(Tier::Sonnet)).is_some());
        assert_eq!(verdict("worker-sonnet", &tooling(), Some(Tier::Sonnet)), None);
        assert_eq!(verdict("worker", &tooling(), Some(Tier::Opus)), None);
        let reasoned = format!("Tier-reason: touches the ledger's own cost model.\n{}", tooling());
        assert_eq!(verdict("worker", &reasoned, Some(Tier::Sonnet)), None);
        let escalated = format!("Follows: worker-haiku run a1b2 failed.\n{}", tooling());
        assert_eq!(verdict("worker", &escalated, Some(Tier::Sonnet)), None);
    }

    #[test]
    fn an_unreadable_pick_and_non_worker_agents_pass() {
        assert_eq!(verdict("worker", &tooling(), None), None);
        assert_eq!(verdict("Explore", &tooling(), Some(Tier::Sonnet)), None);
        assert_eq!(verdict("codex", &tooling(), Some(Tier::Sonnet)), None);
    }

    #[test]
    fn a_worker_brief_missing_its_item_line_is_refused_and_a_full_brief_passes() {
        let without: String = tooling().lines().filter(|line| !line.starts_with("Item:")).collect::<Vec<_>>().join("\n");
        for agent in ["worker", "worker-sonnet", "worker-haiku", "codex"] {
            assert!(verdict(agent, &without, Some(Tier::Opus)).is_some_and(|text| text.starts_with("Missing Item:")), "{agent}");
        }
        assert_eq!(verdict("worker", &tooling(), Some(Tier::Opus)), None);
    }
}

mod placement {
    use super::{FACETS, input};
    use crate::guard::worker_tier::{is_native, spawn_parts};
    use serde_json::json;

    fn placement_refusal(brief: &crate::hook::HookInput) -> Option<String> {
        let (kind, prompt) = spawn_parts(brief)?;
        crate::guard::worker_tier::placement_refusal(kind, is_native(brief), prompt)
    }

    #[test]
    fn a_box_that_builds_waits_or_lands_is_sent_to_a_fleet_session_and_read_only_research_stays_a_subagent() {
        let code = format!("Item: add a flag.\nCategory: tooling\nPayback: enables x; cost 5 min{FACETS}");
        for agent in ["worker", "worker-sonnet", "worker-haiku"] {
            let refused = placement_refusal(&input(agent, json!(true), &code)).unwrap();
            assert!(refused.contains("north fleet launch") && refused.contains(agent), "{refused}");
        }
        let research = format!("Item: survey the ledger.\nCategory: research{FACETS}");
        assert_eq!(placement_refusal(&input("worker", json!(true), &research)), None);
        assert_eq!(placement_refusal(&input("codex", json!(true), &code)), None);
        assert_eq!(placement_refusal(&input("Explore", json!(true), &code)), None);
    }
}
