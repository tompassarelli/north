use super::worker_tier::refusal;
use crate::hook::HookInput;
use serde_json::{Value, json};

const FACETS: &str = "\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test";
const PAYBACK: &str = "Payback: saves 4 min/change (Tom 0.3, agent 1), 20 changes/day, cost 45 min, payback 0.5 days";

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
        (true, "worker-high", docs.clone()),
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
fn decisions_equal_the_recorded_shell_guard_on_254_briefs() {
    let recorded: Vec<Value> = serde_json::from_str(include_str!("fixtures/worker_tier.json")).unwrap();
    assert_eq!(recorded.len(), 254);
    for case in recorded {
        let agent = case["agent"].as_str().unwrap();
        let brief = input(agent, case["background"].clone(), case["prompt"].as_str().unwrap());
        assert_eq!(refusal(&brief).as_deref(), case["reason"].as_str(), "{case}");
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

#[test]
fn a_foreground_spawn_or_facet_synonym_is_repaired_when_nothing_else_is_wrong() {
    use super::worker_repair::repaired;
    let foreground = input("worker", json!(false), &feature());
    let updated = repaired(&foreground).unwrap();
    assert_eq!(updated["run_in_background"], crate::platform::pyjson::Value::Bool(true));
    assert_eq!(updated["prompt"].as_str(), foreground.tool_input["prompt"].as_str());

    let synonyms = feature()
        .replace("Scope: module", "Scope: single-repo")
        .replace("Verify: local-test", "Verify: read-only");
    let updated = repaired(&input("worker", json!(true), &synonyms)).unwrap();
    let prompt = updated["prompt"].as_str().unwrap();
    assert!(prompt.contains("Scope: cross-module") && prompt.contains("Verify: none"), "{prompt}");
    assert_eq!(updated["run_in_background"], crate::platform::pyjson::Value::Bool(true));

    for (agent, background, prompt) in [
        ("worker", json!(false), feature().replace("Spec: measured", "Spec: Peon")),
        ("worker", json!(true), feature().replace("Scope: module", "Scope: read-only")),
        ("worker", json!(false), "Item: x".to_owned()),
        ("worker-high", json!(false), feature()),
    ] {
        assert!(repaired(&input(agent, background, &prompt)).is_none(), "{prompt}");
    }
}
