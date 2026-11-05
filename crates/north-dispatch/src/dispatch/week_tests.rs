use super::slots::Account;
use super::tests::{MAX_5X, accounts_of, chosen, host, sized};
use crate::pacing::placement::gate;

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
