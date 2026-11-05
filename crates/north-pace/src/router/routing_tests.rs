use super::ctx::{Ctx, OVER};
use super::plan::divergence;
use serde_json::{Value, json};

const SCENARIOS: [(&str, &str); 15] = [
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
        "conserve",
        include_str!("fixtures/scenarios/conserve.json"),
    ),
    (
        "deadline-drain",
        include_str!("fixtures/scenarios/deadline-drain.json"),
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
        "pace-report-only",
        include_str!("fixtures/scenarios/pace-report-only.json"),
    ),
    (
        "report-only-floor",
        include_str!("fixtures/scenarios/report-only-floor.json"),
    ),
    (
        "reserve",
        include_str!("fixtures/scenarios/reserve.json"),
    ),
    (
        "thursday-behind",
        include_str!("fixtures/scenarios/thursday-behind.json"),
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

// KEEP(mutant): only test of the router's plans and deadline-drain
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
    let claude = |headroom: f64, reset_in: f64, resets_at: &str| {
        json!({"provider": "claude", "target_left": 5, "usd_per_week": 50, "age_min": 5,
               "windows": {"week": {"headroom": headroom, "window_h": 168, "reset_in": reset_in, "resets_at": resets_at, "as_of": "2026-10-11T12:00Z", "beta": 0}}})
    };
    let over_pace = |week_headroom: f64, five_hour_headroom: Option<f64>| {
        let mut account = claude(week_headroom, 10.0, "2026-10-11T22:00Z");
        if let Some(headroom) = five_hour_headroom {
            account["windows"]["5h"] = json!({"headroom": headroom, "reset_in": 2});
        }
        ctx(json!({
            "now": "2026-10-11T12:00Z", "lease": true, "projects": {"north": {"u": 1}},
            "accounts": {"claude:b": account},
            "billing": {"claude": {"standard": "plan", "overflow": "blocked"}},
            "pace": {"enforce_from": {"claude:b": "2026-10-01T00:00Z"}},
            "tiers": [{"key": "claude:opus-medium", "rank": 0, "start": 0}],
            "boxes": [{"id": "hold", "project": "north", "category": "feature", "difficulty": 95, "sensitivity": "private"}],
        }))
    };
    let plan = |held: &Ctx| {
        let plan = held.plan().unwrap();
        (
            plan.boxes["hold"].tier.clone(),
            plan.boxes["hold"].wait.clone(),
            plan.needs.len(),
        )
    };
    let admitted = over_pace(5.0, None);
    assert_eq!(admitted.route_account("claude"), None);
    assert_eq!(admitted.account_state("claude:b"), Some(OVER));
    assert_eq!(
        plan(&admitted),
        (Some("claude:opus-medium".to_owned()), None, 0)
    );
    assert_eq!(
        plan(&over_pace(5.0, Some(6.0))),
        (Some("claude:opus-medium".to_owned()), None, 0)
    );
    assert_eq!(
        plan(&over_pace(0.0, None)),
        (None, Some("account exhausted".to_owned()), 0)
    );
    assert_eq!(
        plan(&over_pace(5.0, Some(5.0))),
        (None, Some("account exhausted".to_owned()), 0)
    );
    assert_eq!(
        over_pace(5.0, Some(5.0))
            .launch_refusal("claude:b")
            .as_deref(),
        Some("exhausted: 5h 95% >= 95%")
    );
}

