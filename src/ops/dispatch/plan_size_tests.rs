use super::slots::{PlanSize, Room};
use super::tests::{accounts_of, chosen, host, MAX_20X, MAX_5X, sized};

#[test]
fn plan_size_is_read_per_window_from_the_catalog_so_a_max_20x_holds_four_times_the_five_hour_room_but_twice_the_week_of_a_max_5x() {
    let cfg: toml_edit::DocumentMut = "[accounts.claude]\nplans = { a = \"max-20x\", c = \"max-5x\", p = \"pro\" }".parse().unwrap();
    let catalog: toml_edit::DocumentMut = super::survey::CATALOG.parse().unwrap();
    let plans: Vec<Result<PlanSize, String>> = ["claude:a", "claude:c"].iter().map(|name| super::survey::plan(&cfg, &catalog, name)).collect();
    assert_eq!(plans, [Ok(MAX_20X), Ok(MAX_5X)]);
    assert!(super::survey::plan(&cfg, &catalog, "claude:b").is_err(), "an account with no plan is named in a note");
    assert!(super::survey::plan(&cfg, &catalog, "claude:p").is_err(), "a plan the catalog has no multipliers for is named in a note");
    let week = |left: f64| Room { five_hour: 50.0, week: left, week_reset_h: 100.0 };
    let accounts = vec![sized("claude:a", &["gex45"], MAX_20X, week(20.0)), sized("claude:c", &["gex45"], MAX_5X, week(50.0))];
    assert_eq!(accounts_of(&chosen(vec![host("gex45", 24, Ok(23))], accounts, 1)), ["claude:c"], "a fifth of a Max 20x week is two fifths of a Max 5x week, not four");
    let tied_week = |five_hour: f64, left: f64| Room { five_hour, week: left, week_reset_h: 100.0 };
    let accounts = vec![sized("claude:a", &["gex45"], MAX_20X, tied_week(20.0, 20.0)), sized("claude:c", &["gex45"], MAX_5X, tied_week(50.0, 40.0))];
    assert_eq!(accounts_of(&chosen(vec![host("gex45", 24, Ok(23))], accounts, 1)), ["claude:a"], "equal week units leave the five-hour room, where a fifth of a Max 20x is four times as large");
}

#[test]
fn an_account_ranked_first_by_expiring_week_room_but_short_of_the_roomiest_headroom_launches_pinned() {
    let soon = Room { five_hour: 50.0, week: 30.0, week_reset_h: 10.0 };
    let late = Room { five_hour: 80.0, week: 70.0, week_reset_h: 150.0 };
    let accounts = vec![sized("claude:a", &["gex45"], MAX_5X, soon), sized("claude:b", &["gex45"], MAX_5X, late)];
    let picks = chosen(vec![host("gex45", 24, Ok(0))], accounts, 1);
    assert_eq!(accounts_of(&picks), ["claude:a"], "a's week expires sooner, so dispatch spends it first");
    let pin = picks[0].1.as_deref().unwrap_or_default();
    assert!(pin.contains("placement would pick claude:b for its 70% headroom over 30%"), "{pin:?}");
    let roomiest = vec![sized("claude:a", &["gex45"], MAX_5X, Room { five_hour: 80.0, ..soon }), sized("claude:b", &["gex45"], MAX_5X, Room { five_hour: 20.0, ..late })];
    assert_eq!(chosen(vec![host("gex45", 24, Ok(0))], roomiest, 1), [("claude:a".to_owned(), None)], "a pick that is also the roomiest needs no pin");
}
