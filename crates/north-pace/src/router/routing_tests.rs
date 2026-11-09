use super::ctx::Ctx;
use super::plan::divergence;
use serde_json::{Value, json};

const SCENARIOS: [(&str, &str); 9] = [
    (
        "burn",
        include_str!("fixtures/scenarios/burn.json"),
    ),
    (
        "capacity-astra",
        include_str!("fixtures/scenarios/capacity-astra.json"),
    ),
    (
        "capacity-floor",
        include_str!("fixtures/scenarios/capacity-floor.json"),
    ),
    (
        "codex-explore",
        include_str!("fixtures/scenarios/codex-explore.json"),
    ),
    (
        "codex-gate",
        include_str!("fixtures/scenarios/codex-gate.json"),
    ),
    (
        "deadline",
        include_str!("fixtures/scenarios/deadline.json"),
    ),
    (
        "measured",
        include_str!("fixtures/scenarios/measured.json"),
    ),
    (
        "new-model",
        include_str!("fixtures/scenarios/new-model.json"),
    ),
    (
        "two-claude",
        include_str!("fixtures/scenarios/two-claude.json"),
    ),
];

fn scenario(name: &str) -> Value {
    serde_json::from_str(SCENARIOS.iter().find(|(n, _)| *n == name).unwrap().1).unwrap()
}

fn ctx(value: Value) -> Ctx {
    serde_json::from_value(value).unwrap()
}

#[test]
fn recorded_scenarios_reach_their_expected_plans() {
    for (name, text) in SCENARIOS {
        let raw: Value = serde_json::from_str(text).unwrap();
        let plan = ctx(raw.clone()).plan().unwrap();
        let shown = serde_json::to_value(&plan).unwrap();
        assert_eq!(divergence(&shown, &raw["expect"]), None, "{name}");
    }
    let mut astra = ctx(scenario("capacity-astra"));
    astra.tiers.push(serde_json::from_value(json!({"key": "codex:astra-xhigh", "model": "gpt-6-astra", "effort": "xhigh", "rank": 5, "start": 90})).unwrap());
    astra.boxes.push(serde_json::from_value(json!({"id": "x1", "project": "galileo", "category": "feature", "difficulty": 95, "sensitivity": "private",
        "brief": "Category: feature\nSpec: exact\nScope: module\nSurface: shell\nVerify: local-test\nDone when: `bash t.test.sh`\nETA: 15 min\nFiles: harness/t.sh"})).unwrap());
    astra
        .knobs
        .codex
        .codex_tier
        .get_mut("feature")
        .unwrap()
        .push("codex:astra-xhigh".into());
    let plan = astra.plan().unwrap();
    assert_eq!(
        (
            plan.boxes["x1"].tier.as_deref(),
            plan.boxes["x1"].fallback,
            plan.boxes["f1"].fallback
        ),
        (Some("codex:astra-xhigh"), false, true)
    );
    let limited = |week_headroom: f64, five_hour_headroom: Option<f64>| {
        let mut account = json!({"provider": "claude", "usd_per_week": 50,
            "windows": {"week": {"headroom": week_headroom, "reset_in": 10.0, "resets_at": "2026-10-11T22:00Z"}}});
        if let Some(headroom) = five_hour_headroom {
            account["windows"]["5h"] = json!({"headroom": headroom, "reset_in": 2});
        }
        ctx(json!({
            "now": "2026-10-11T12:00Z", "lease": true, "projects": {"north": {"u": 1}},
            "accounts": {"claude:b": account},
            "billing": {"claude": {"standard": "plan", "overflow": "blocked"}},
            "tiers": [{"key": "claude:opus-medium", "rank": 0, "start": 0}],
            "boxes": [{"id": "hold", "project": "north", "category": "feature", "difficulty": 95, "sensitivity": "private"}],
        }))
    };
    let plan = |held: &Ctx| {
        let plan = held.plan().unwrap();
        (plan.boxes["hold"].tier.clone(), plan.boxes["hold"].wait.clone(), plan.needs.len())
    };
    let admitted = (Some("claude:opus-medium".to_owned()), None, 0);
    let exhausted = (None, Some("account exhausted".to_owned()), 0);
    assert_eq!(plan(&limited(5.0, Some(1.0))), admitted);
    assert_eq!(plan(&limited(0.0, None)), exhausted);
    assert_eq!(plan(&limited(5.0, Some(0.0))), exhausted);
    assert_eq!(limited(5.0, Some(0.0)).provider_limit("claude:b").as_deref(), Some("claude:b is at its provider's 5h limit"));
}
