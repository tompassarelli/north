use super::slots::Account;
use super::tests::{MAX_5X, accounts_of, chosen, host, sized};
use crate::ops::pacing::placement::gate;

#[test]
fn an_account_with_one_point_left_in_the_week_is_out_and_never_chosen_for_a_launch() {
    let usage = |week: f64| serde_json::json!({"windows": [{"window": "5h", "used_pct": 10.0, "burn_pct_h": 1.0}, {"window": "week", "used_pct": week, "burn_pct_h": 0.0}]});
    let control = serde_json::json!({"at": 1000.0});
    let judged = |name: &str, week: f64| {
        let (usage, at) = (usage(week), 1060.0);
        let room = super::survey::room(&usage, at);
        let refusal = super::survey::account_refusal(name, &control, Some(&gate::verdict(name, &usage)), &room, at);
        Account { refusal, ..sized(name, &["gex45"], MAX_5X, room) }
    };
    let (spent, spare) = (judged("claude:c", 99.0), judged("claude:b", 90.0));
    let why = spent.open().unwrap_err();
    assert!(why.contains("1 points left in the week"), "{why}");
    assert!(spent.line().contains("out: claude:c unavailable"), "{}", spent.line());
    assert_eq!(spare.open(), Ok(()));
    let launches = chosen(vec![host("gex45", 24, Ok(0))], vec![spent, spare], 3);
    assert_eq!(accounts_of(&launches), ["claude:b"; 3], "{launches:?}");
}

#[test]
fn a_launch_refused_for_the_week_shows_that_refusal_in_dispatchs_failure_line_not_the_tier_note() {
    let stderr = "tier: north#295 runs worker on claude-opus-5-5 (the brief names worker)\nplacement: claude:d\n  claude:c unavailable: exhausted: provider week 100%\nerror: claude:c unavailable: exhausted: provider week 100%; placement picks claude:d\n";
    assert_eq!(super::effects::refusal_line(stderr), "claude:c unavailable: exhausted: provider week 100%; placement picks claude:d");
    assert_eq!(super::effects::refusal_line("tier: a\nlaunch could not start\n"), "launch could not start");
    assert_eq!(super::effects::refusal_line(""), "failed");
}
