use super::brief::{codex_explore_qualifies, codex_qualifies};
use super::choose::Bands;
use super::ctx::{Account, Ctx, Tier, WorkBox, exhausted};
use super::moment::Moment;
use serde_json::{Value, json};

const NOW: &str = "2026-10-17T03:00Z";
const RESET: &str = "2026-10-17T04:00Z";

fn ctx(value: Value) -> Ctx {
    serde_json::from_value(value).unwrap()
}

fn account(headroom: f64, provider: &str) -> Value {
    json!({"provider": provider, "age_min": 5, "target_left": 5, "usd_per_week": 100,
           "windows": {"week": {"headroom": headroom, "window_h": 168, "reset_in": 1, "beta": 0, "resets_at": RESET, "as_of": NOW}}})
}

fn plan_billing() -> Value {
    json!({"claude": {"standard": "plan", "fast": "plan", "overflow": "money"}, "codex": {"standard": "plan"}})
}

fn tier(key: &str, rank: f64, start: f64) -> Tier {
    serde_json::from_value(json!({"key": key, "rank": rank, "start": start})).unwrap()
}

fn item(category: &str, difficulty: f64, sensitivity: &str) -> WorkBox {
    serde_json::from_value(json!({"id": "b", "project": "p", "category": category, "difficulty": difficulty, "sensitivity": sensitivity})).unwrap()
}

#[test]
fn a_box_at_a_band_boundary_goes_to_the_higher_tier_only() {
    let ctx = ctx(json!({"now": NOW, "tiers": [
        {"key": "claude:low", "rank": 0, "start": 0}, {"key": "claude:high", "rank": 1, "start": 50}]}));
    let bands = ctx.bands();
    let (low, high) = (&ctx.tiers[0], &ctx.tiers[1]);
    let at = item("feature", 50.0, "private");
    assert!(!ctx.admit(low, &at, &bands));
    assert!(ctx.admit(high, &at, &bands));
    let below = item("feature", 49.0, "private");
    assert!(ctx.admit(low, &below, &bands));
    assert!(!ctx.admit(high, &below, &bands));
}

#[test]
fn two_admitted_tiers_on_one_account_choose_the_lower_ranked() {
    let ctx = ctx(json!({"now": NOW, "billing": plan_billing(), "accounts": {"claude": account(90.0, "claude")},
        "tiers": [{"key": "claude:big", "rank": 1, "start": 0}, {"key": "claude:small", "rank": 0, "start": 0}],
        "success": {"claude:big|feature": 100, "claude:small|feature": 100}}));
    let chosen = ctx.choose(&item("feature", 50.0, "private"), false);
    assert_eq!(chosen.tier.unwrap().key, "claude:small");
}

#[test]
fn an_account_whose_week_headroom_equals_its_reserve_is_exhausted() {
    let acct = |headroom: f64| -> Account { serde_json::from_value(account(headroom, "claude")).unwrap() };
    assert!(exhausted(Some(&acct(5.0))));
    assert!(!exhausted(Some(&acct(5.1))));
    assert!(!exhausted(None));
}

#[test]
fn fast_needs_headroom_strictly_above_the_fast_floor() {
    let fast = |headroom: f64| {
        let ctx = ctx(json!({"now": NOW, "billing": plan_billing(), "accounts": {"claude": account(headroom, "claude")}}));
        ctx.fast("claude")
    };
    assert!(!fast(10.0));
    assert!(fast(11.0));
}

#[test]
fn a_codex_capacity_hit_falls_back_to_astra_only_with_the_floor_left_after_its_cost() {
    let fallback = |headroom: f64| {
        let since = Moment::parse(NOW).unwrap().plus_seconds(-5.0 * 60.0).iso();
        let ctx = ctx(json!({"now": NOW, "billing": plan_billing(), "accounts": {"codex": account(headroom, "codex")},
            "capacity": {"codex": {"since": since, "model": "gpt-6.1-sol"}}}));
        ctx.capacity().get("codex").map(|hit| hit.fallback.is_some())
    };
    assert_eq!(fallback(15.0), Some(true));
    assert_eq!(fallback(14.0), Some(false));
}

#[test]
fn a_capacity_hit_older_than_the_capacity_window_is_forgotten() {
    let held = |minutes_ago: f64| {
        let since = Moment::parse(NOW).unwrap().plus_seconds(-minutes_ago * 60.0).iso();
        let ctx = ctx(json!({"now": NOW, "billing": plan_billing(), "accounts": {"codex": account(50.0, "codex")},
            "capacity": {"codex": {"since": since, "model": "gpt-6.1-sol"}}}));
        ctx.capacity().contains_key("codex")
    };
    assert!(held(14.0));
    assert!(!held(16.0));
}

#[test]
fn a_codex_tier_outside_the_category_class_is_refused_even_for_a_qualifying_brief() {
    let brief = "Category: feature\nSpec: exact\nScope: module\nSurface: shell\nVerify: local-test\nDone when: `bash t.test.sh`\nETA: 20 min\nFiles: a.sh\n";
    let ctx = ctx(json!({"now": NOW, "success": {"codex:sol-medium|feature": 100, "codex:sol-high|feature": 100}}));
    let mut work = item("feature", 50.0, "private");
    work.brief = Some(brief.to_owned());
    let (medium, high) = (tier("codex:sol-medium", 0.0, 0.0), tier("codex:sol-high", 1.0, 0.0));
    assert!(ctx.admit(&medium, &work, &Bands::new()));
    assert!(!ctx.admit(&high, &work, &Bands::new()));
}

#[test]
fn a_surface_of_assets_qualifies_codex_only_for_the_explore_shape() {
    let ctx = ctx(json!({"now": NOW}));
    let brief = "Category: tooling\nSpec: exact\nScope: module\nSurface: assets\nVerify: local-test\nDone when: `bash t.test.sh`\nETA: 20 min\nFiles: a.sh\n";
    assert_eq!(codex_qualifies(brief, false, &ctx.knobs.codex), (false, Some("Surface assets".into())));
    assert!(codex_explore_qualifies(brief, &ctx.knobs.codex).0);
}

#[test]
fn spend_at_the_money_ceiling_with_a_billable_overflow_prices_the_account_out() {
    let price = |headroom: f64| {
        let ctx = ctx(json!({"now": NOW, "billing": plan_billing(), "accounts": {"claude": account(headroom, "claude")}}));
        ctx.price("claude")
    };
    assert_eq!(price(9.0), 0.0);
    assert_eq!(price(8.0), f64::INFINITY);
}

#[test]
fn a_private_box_is_never_admitted_to_a_gemini_tier() {
    let ctx = ctx(json!({"now": NOW, "success": {"gemini:pro|feature": 100}}));
    let gemini = tier("gemini:pro", 0.0, 0.0);
    assert!(!ctx.admit(&gemini, &item("feature", 50.0, "private"), &Bands::new()));
    assert!(ctx.admit(&gemini, &item("feature", 50.0, "public"), &Bands::new()));
}
