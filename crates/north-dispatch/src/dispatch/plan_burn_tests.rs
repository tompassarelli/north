use super::issues::IssueRef;
use super::plan::{Candidate, Capacity, Cloud, Ready, place};
use super::slots::{Account, Pools, Reading, Room};
use super::tests::{host, sized, MAX_5X};
use super::tier::{Route, Tier};

fn ready(eta_h: Option<f64>) -> Ready {
    let route = Route { tier: Tier::Sonnet, category: "tooling".into(), arm: false, effort: None, effort_arm: false, follows: None, why: "tooling arm sonnet-high".into() };
    Ready { item: IssueRef::new("north", 259), brief: "brief".into(), code_only: false, route, eta_h }
}

fn account(name: &str, five_hour_left: f64, burn: Option<f64>) -> Account {
    Account { burn, ..sized(name, &["gex45"], MAX_5X, Room { five_hour: five_hour_left, week: 90.0, week_reset_h: 100.0 }) }
}

fn weekly(name: &str, week_used: f64, week_burn: f64, reset_h: f64) -> Account {
    let room = Room { five_hour: 80.0, week: 100.0 - week_used, week_reset_h: reset_h };
    let readings = vec![Reading { window: "5h".into(), used_pct: 15.0, burn_pct_h: Some(2.0), reset_h: Some(3.0) }, Reading { window: "week".into(), used_pct: week_used, burn_pct_h: Some(week_burn), reset_h: Some(reset_h) }];
    Account { readings, ..sized(name, &["gex45"], MAX_5X, room) }
}

fn placed_launch(accounts: &[Account], eta_h: Option<f64>) -> Result<super::plan::Launch, String> {
    let hosts = [host("gex45", 24, Ok(22))];
    let cloud = Cloud::Closed("no credit".into());
    let capacity = Capacity { here: "gex45", cloud: &cloud, accounts, charge: 1.0 };
    let candidates = [Candidate { host: "gex45".into(), refusal: None }];
    place(&ready(eta_h), &capacity, &candidates, &mut Pools::new(&hosts, accounts))
}

fn placed(accounts: &[Account], eta_h: Option<f64>) -> Result<(String, Option<String>), String> {
    placed_launch(accounts, eta_h).map(|launch| (launch.account, launch.burn))
}

#[test]
fn a_launch_whose_eta_at_the_accounts_measured_burn_would_cross_its_five_hour_stop_moves_to_an_account_that_fits_and_names_the_burn() {
    let burning = account("claude:b", 10.0, Some(12.0));
    let steady = account("claude:a", 8.0, Some(2.0));
    let (account, burn) = placed(&[burning.clone(), steady.clone()], Some(1.0)).unwrap();
    assert_eq!(account, "claude:a", "b's 12 points over an hour pass its 10 left; a's 2 of 8 do not");
    assert_eq!(burn.unwrap(), "claude:a measured burn 2.0%/h over a 60 min ETA spends 2.0 of the 8 points left to its 5h stop");
    assert_eq!(placed(&[burning, steady], Some(0.5)).unwrap().0, "claude:b", "half an hour at 12%/h spends 6 of b's 10, and b has the room that ranks higher");
}

#[test]
fn a_launch_no_account_can_finish_before_its_stop_is_held_with_every_burn_named() {
    let held = placed(&[account("claude:b", 10.0, Some(12.0))], Some(1.0)).unwrap_err();
    assert!(held.contains("claude:b measured burn 12.0%/h"), "{held}");
}

#[test]
fn an_unmeasured_burn_or_an_unknown_eta_never_holds_a_launch() {
    assert!(placed(&[account("claude:b", 10.0, None)], Some(1.0)).is_ok());
    assert_eq!(placed(&[account("claude:b", 10.0, Some(99.0))], None).unwrap().1, None);
}

#[test]
fn an_account_whose_week_is_projected_past_exhaustion_before_its_reset_is_refused_and_named() {
    let held = placed_launch(&[weekly("claude:a", 88.0, 3.0, 20.0)], Some(4.0)).unwrap_err();
    assert_eq!(held, "gex45 holds the box: claude:a week projected 101% at 3.0%/h over 240 min before its reset, past 100%");
    let carried = placed_launch(&[weekly("claude:a", 88.0, 3.0, 20.0), weekly("claude:b", 60.0, 3.0, 20.0)], Some(4.0)).unwrap();
    assert_eq!(carried.account, "claude:b", "b's week reaches 73% by the ETA");
    assert!(placed_launch(&[weekly("claude:a", 88.0, 3.0, 2.0)], Some(4.0)).is_ok(), "a's week resets two hours in, so it carries the box");
}

#[test]
fn a_placed_launch_carries_its_accounts_windows_projected_at_its_eta() {
    let launch = placed_launch(&[weekly("claude:b", 60.0, 1.5, 20.0)], Some(2.0)).unwrap();
    let projection = launch.projection.unwrap();
    assert_eq!(projection.eta_h, 2.0);
    let shown: Vec<(&str, f64, f64, bool)> = projection.windows.iter().map(|w| (w.window.as_str(), w.used_pct, w.projected_pct, w.at_eta)).collect();
    assert_eq!(shown, [("5h", 15.0, 19.0, true), ("week", 60.0, 63.0, true)]);
    assert_eq!(placed_launch(&[weekly("claude:b", 60.0, 1.5, 20.0)], None).unwrap().projection, None);
}
