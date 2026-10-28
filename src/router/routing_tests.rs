use super::brief::codex_box_refusal;
use super::ctx::{Ctx, Knobs, OVER, Project, UNDER};
use super::moment::{Moment, python_g};
use super::ops::answer;
use super::plan::divergence;
use super::route::drain_open;
use serde_json::{Value, json};

const SCENARIOS: [(&str, &str); 15] = [
    (
        "burn",
        include_str!("../../harness/lib/routing-scenarios/burn.json"),
    ),
    (
        "capacity-astra",
        include_str!("../../harness/lib/routing-scenarios/capacity-astra.json"),
    ),
    (
        "capacity-floor",
        include_str!("../../harness/lib/routing-scenarios/capacity-floor.json"),
    ),
    (
        "codex-explore",
        include_str!("../../harness/lib/routing-scenarios/codex-explore.json"),
    ),
    (
        "codex-gate",
        include_str!("../../harness/lib/routing-scenarios/codex-gate.json"),
    ),
    (
        "conserve",
        include_str!("../../harness/lib/routing-scenarios/conserve.json"),
    ),
    (
        "deadline-drain",
        include_str!("../../harness/lib/routing-scenarios/deadline-drain.json"),
    ),
    (
        "deadline",
        include_str!("../../harness/lib/routing-scenarios/deadline.json"),
    ),
    (
        "measured",
        include_str!("../../harness/lib/routing-scenarios/measured.json"),
    ),
    (
        "new-model",
        include_str!("../../harness/lib/routing-scenarios/new-model.json"),
    ),
    (
        "pace-report-only",
        include_str!("../../harness/lib/routing-scenarios/pace-report-only.json"),
    ),
    (
        "report-only-floor",
        include_str!("../../harness/lib/routing-scenarios/report-only-floor.json"),
    ),
    (
        "reserve",
        include_str!("../../harness/lib/routing-scenarios/reserve.json"),
    ),
    (
        "thursday-behind",
        include_str!("../../harness/lib/routing-scenarios/thursday-behind.json"),
    ),
    (
        "two-claude",
        include_str!("../../harness/lib/routing-scenarios/two-claude.json"),
    ),
];

fn scenario(name: &str) -> Value {
    serde_json::from_str(SCENARIOS.iter().find(|(n, _)| *n == name).unwrap().1).unwrap()
}

fn ctx(value: Value) -> Ctx {
    serde_json::from_value(value).unwrap()
}

fn week(headroom: f64, resets_at: &str, as_of: &str) -> Value {
    json!({"headroom": headroom, "window_h": 168, "reset_in": 20, "beta": 0, "resets_at": resets_at, "as_of": as_of})
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

#[test]
fn codex_paces_to_friday_2359_eastern_across_dst() {
    for (reset, friday) in [
        ("2026-10-17T04:00:00Z", "2026-10-16T23:59:00-04:00"),
        ("2026-11-07T05:00:00Z", "2026-11-06T23:59:00-05:00"),
    ] {
        let start = Moment::parse(reset).unwrap().plus_seconds(-168.0 * 3600.0);
        let cutoff = Moment::parse(friday).unwrap();
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let at = start.plus_seconds(cutoff.seconds_since(start) * fraction);
            let wanted = 85.0 * fraction;
            for delta in [-1.0, 0.0, 1.0] {
                if !(0.0..=100.0).contains(&(wanted + delta)) {
                    continue;
                }
                let acct = json!({"provider": "codex", "target_left": 15, "usd_per_week": 50,
                                  "windows": {"week": week(100.0 - wanted - delta, reset, &at.iso())}});
                let ctx = ctx(json!({"now": at.iso(), "accounts": {"codex": acct}}));
                let m = ctx.metrics(&ctx.accounts["codex"]);
                assert_eq!(m.pace_at.as_deref(), Some(friday), "{reset}");
                assert!((m.pace_pct.unwrap() - wanted).abs() < 1e-9, "{reset} {fraction} {m:?}");
                assert!((m.available_usd - (100.0 - wanted - delta) / 2.0).abs() < 1e-9, "{m:?}");
                assert_eq!(m.state, if delta < 0.0 { UNDER } else { OVER }, "{reset} {fraction} {delta}");
            }
        }
    }
}

#[test]
fn claude_paces_to_its_reset_on_a_five_percent_floor() {
    let reset = "2026-10-17T04:00:00Z";
    let start = Moment::parse(reset).unwrap().plus_seconds(-168.0 * 3600.0);
    for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let at = start.plus_seconds(168.0 * 3600.0 * fraction);
        let wanted = 95.0 * fraction;
        for delta in [-1.0, 0.0, 1.0] {
            if !(0.0..=100.0).contains(&(wanted + delta)) {
                continue;
            }
            let acct = json!({"provider": "claude", "target_left": 5, "usd_per_week": 50,
                              "windows": {"week": week(100.0 - wanted - delta, reset, &at.iso())}});
            let ctx = ctx(json!({"now": at.iso(), "accounts": {"claude": acct}}));
            let m = ctx.metrics(&ctx.accounts["claude"]);
            assert!((m.pace_pct.unwrap() - wanted).abs() < 1e-9, "{fraction} {m:?}");
            assert!((m.available_usd - (100.0 - wanted - delta) / 2.0).abs() < 1e-9, "{m:?}");
            assert_eq!(m.state, if delta < 0.0 { UNDER } else { OVER }, "{fraction} {delta}");
        }
    }
}

#[test]
fn a_named_account_reads_its_own_pace_not_the_routed_one() {
    let mut raw = scenario("capacity-astra");
    let mut a = raw["accounts"]["claude"].clone();
    let mut b = a.clone();
    a["windows"]["week"]["headroom"] = json!(33);
    b["windows"]["week"]["headroom"] = json!(90);
    b["windows"]["week"]["reset_in"] = json!(10);
    raw["accounts"] = json!({"claude:a": a, "claude:b": b});
    let ctx = ctx(raw);
    assert_eq!(ctx.route_account("claude").as_deref(), Some("claude:b"));
    let named = ctx.pace_reading("claude", Some("claude:a")).unwrap();
    assert_eq!(
        (
            named["account"].as_str(),
            named["state"].as_str(),
            named["used_pct"].as_f64()
        ),
        (Some("claude:a"), Some(OVER), Some(67.0))
    );
    assert_eq!(
        ctx.pace_reading("claude", Some("claude:b")).unwrap()["state"],
        UNDER
    );
    assert_eq!(
        ctx.pace_reading("claude", None).unwrap()["account"],
        "claude:b"
    );
}

#[test]
fn a_drain_needs_a_grant_and_a_target_open_through_its_day() {
    let now = Moment::parse("2026-10-20T23:00:00Z").unwrap();
    let project = |target: &str, grant: &str| -> Project {
        serde_json::from_value(json!({"target": target, "drain": {"codex": grant}})).unwrap()
    };
    assert!(drain_open(
        &project("2026-10-20", "Tom, 12 Oct"),
        "codex:a",
        now
    ));
    assert!(drain_open(&project("14 Oct", "Tom, 12 Oct"), "codex", now));
    assert!(!drain_open(
        &project("2026-10-19", "Tom, 12 Oct"),
        "codex",
        now
    ));
    assert!(!drain_open(
        &project("2026-10-20T22:00Z", "Tom, 12 Oct"),
        "codex",
        now
    ));
    assert!(!drain_open(&project("2026-10-20", "  "), "codex", now));
    assert!(!drain_open(
        &project("2026-10-20", "Tom, 12 Oct"),
        "claude",
        now
    ));
}

#[test]
fn money_refusal_says_why_a_new_worker_could_bill_money() {
    let fresh = |headroom: f64, extra: Value| {
        let mut acct = json!({"age_min": 5, "target_left": 5, "windows": {"week": week(headroom, "2026-10-17T04:00Z", "2026-10-15T12:00Z")}});
        acct.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        acct
    };
    let ctx = ctx(json!({
        "now": "2026-10-15T12:00Z",
        "accounts": {"codex": fresh(90.0, json!({})), "claude": fresh(5.0, json!({"hasCredits": true, "spend": 1})),
                     "gemini": fresh(90.0, json!({})), "old": fresh(90.0, json!({"age_min": 45.4}))},
        "billing": {"codex": {"standard": "plan"}, "claude": {"standard": "plan", "overflow": "blocked"},
                    "gemini": {"standard": "money"}, "old": {"standard": "plan"}},
    }));
    assert_eq!(ctx.money_refusal("codex"), None);
    assert_eq!(
        ctx.money_refusal("claude").unwrap(),
        "claude is at 95% of a plan window (ceiling 92%) and hasCredits, spend would bill money past it"
    );
    assert_eq!(
        ctx.money_refusal("gemini").unwrap(),
        "the catalog bills gemini's standard usage as money"
    );
    assert_eq!(
        ctx.money_refusal("old").unwrap(),
        "old's usage snapshot is 45 minutes old even after a refresh"
    );
    assert!(
        ctx.money_refusal("nobody")
            .unwrap()
            .starts_with("no usage snapshot for nobody")
    );
}

#[test]
fn codex_box_holds_reviews_to_another_family_and_explore_to_its_shape() {
    let codex = Knobs::default().codex;
    let review = |author: &str| format!("Category: review\nReviews: {author}\nSpec: judged\n");
    assert_eq!(
        codex_box_refusal(&review("claude (opus-high netcode plan)"), &codex),
        None
    );
    assert_eq!(
        codex_box_refusal(&review("codex"), &codex).as_deref(),
        Some("Reviews codex")
    );
    assert_eq!(
        codex_box_refusal("Category: review\n", &codex).as_deref(),
        Some("Reviews missing")
    );
    let bug = "Category: bug-known-cause\nSpec: exact\nScope: module\nSurface: assets\nVerify: visual\nDone when: `bash t.test.sh`\nETA: 15 min\nFiles: harness/t.sh\n";
    assert_eq!(
        codex_box_refusal(bug, &codex).as_deref(),
        Some("Category bug-known-cause")
    );
    assert_eq!(
        codex_box_refusal(&format!("Explore: codex\n{bug}"), &codex),
        None
    );
    assert_eq!(
        codex_box_refusal(
            &format!("Explore: codex\n{}", bug.replace("assets", "lua")),
            &codex
        )
        .as_deref(),
        Some("Surface lua")
    );
    assert_eq!(codex_box_refusal("Item: north#1\n", &codex), None);
}

#[test]
fn plan_lines_tell_the_lead_to_write_explore_codex() {
    let ctx = ctx(scenario("codex-explore"));
    let result = serde_json::to_value(ctx.plan().unwrap()).unwrap();
    let lines = ctx.box_lines(&result);
    let line = |id: &str| {
        lines
            .iter()
            .find(|l| l.starts_with(&format!("box {id} ")))
            .unwrap()
            .clone()
    };
    assert!(
        line("b0").contains("(explore: brief header Explore: codex)"),
        "{lines:?}"
    );
    assert!(!line("b10").contains("Explore: codex"), "{lines:?}");
}

#[test]
fn service_prices_print_like_python_format_g() {
    let cases = [
        (12.5, "12.5"),
        (7.0, "7"),
        (100000.0, "100000"),
        (1234567.0, "1.23457e+06"),
        (0.0001, "0.0001"),
        (0.00001234, "1.234e-05"),
    ];
    for (value, shown) in cases {
        assert_eq!(python_g(value), shown);
    }
}

#[test]
fn pace_ops_answer_router_requests_and_leave_pace_ops_alone() {
    let refused = answer(&json!({"op": "codex_box", "brief": "Category: research\nSpec: exact\n"}));
    assert_eq!(refused, Some(Ok(json!("Category research"))));
    let drain = json!({"op": "drain", "project": {"target": "14 Oct", "drain": {"codex": "Tom"}}, "account": "codex", "now": "2026-10-16 12:00:00+00:00"});
    assert_eq!(
        answer(&drain),
        Some(Ok(json!({"drain_open": true, "target_open": true})))
    );
    assert_eq!(answer(&json!({"op": "advance"})), None);
    assert!(
        matches!(answer(&json!({"op": "plan", "ctx": {}})), Some(Err(error)) if error.contains("now"))
    );
}

#[test]
fn a_codex_tier_admits_by_the_brief_category_and_explores_within_the_shape_it_may() {
    let base = "Item: north#51\nCategory: tooling\nSpec: exact\nScope: module\nSurface: shell\nVerify: local-test\nDone when: `bash t.test.sh`\nETA: 20 min\nFiles: a.sh, b.sh\n";
    let tier = |key: &str, extra: Value| -> super::ctx::Tier {
        let mut tier = json!({"key": key, "model": "m", "effort": "high"});
        tier.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(tier).unwrap()
    };
    let item = |category: &str, brief: String| -> super::ctx::WorkBox {
        serde_json::from_value(json!({"id": "b", "project": "north", "category": category, "difficulty": 50, "sensitivity": "public", "brief": brief})).unwrap()
    };
    let (sol_medium, sol_high) = (tier("codex:sol-medium", json!({})), tier("codex:sol-high", json!({})));
    let admit = ctx(json!({"now": "2026-10-15T12:00Z", "success": {"codex:sol-medium|tooling": 100, "codex:sol-high|tooling": 100}}));
    let feature = item("tooling", base.replace("tooling", "feature"));
    assert!(admit.admit(&sol_medium, &feature, &Default::default()));
    assert!(!admit.admit(&sol_high, &feature, &Default::default()));

    let under = ctx(json!({"now": "2026-10-15T12:00Z", "accounts": {"codex": {
        "windows": {"week": {"headroom": 90, "window_h": 168, "reset_in": 80, "beta": 0, "as_of": "2026-10-15T12:00Z", "resets_at": "2026-10-17T04:00Z"}},
        "provider": "codex", "target_left": 15, "usd_per_week": 50, "age_min": 5}}}));
    assert_eq!(under.account_state("codex"), Some(UNDER));
    let bug = item("bug-known-cause", base.replace("tooling", "bug-known-cause"));
    assert!(under.explorable(&sol_medium, &bug));
    assert!(!under.explorable(&sol_high, &bug));
    let explored = tier("codex:sol-medium", json!({"class_runs": {"bug-known-cause": 20}}));
    assert!(!under.explorable(&explored, &bug));
    let in_ts = item("bug-known-cause", bug.brief.clone().unwrap().replace("shell", "ts"));
    assert!(!under.explorable(&sol_medium, &in_ts));
    let research = item("bug-known-cause", bug.brief.clone().unwrap().replace("bug-known-cause", "research"));
    assert!(!under.explorable(&sol_medium, &research));
    for category in ["performance", "netcode-determinism", "balance-tuning"] {
        let facing = item("bug-known-cause", bug.brief.clone().unwrap().replace("bug-known-cause", category));
        assert!(under.explorable(&sol_high, &facing), "{category}");
        assert!(!under.explorable(&sol_medium, &facing), "{category}");
        assert!(!admit.admit(&sol_high, &facing, &Default::default()), "{category}");
        let visual = item("bug-known-cause", facing.brief.clone().unwrap().replace("Surface: shell", "Surface: assets").replace("Verify: local-test", "Verify: visual"));
        assert!(under.explorable(&sol_high, &visual), "{category}");
    }
    let lua = item("bug-known-cause", bug.brief.clone().unwrap().replace("shell", "lua"));
    assert!(!under.explorable(&sol_medium, &lua));
}
