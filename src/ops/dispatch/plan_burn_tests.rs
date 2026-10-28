use super::issues::IssueRef;
use super::plan::{Candidate, Capacity, Cloud, Ready, place};
use super::slots::{Account, Pools, Room};
use super::tests::{host, sized, MAX_5X};
use super::tier::{Route, Tier};

fn ready(eta_h: Option<f64>) -> Ready {
    let route = Route { tier: Tier::Sonnet, category: "tooling".into(), arm: false, follows: None, why: "tooling arm sonnet-high".into() };
    Ready { item: IssueRef::new("north", 259), brief: "brief".into(), code_only: false, route, eta_h }
}

fn account(name: &str, five_hour_left: f64, burn: Option<f64>) -> Account {
    Account { burn, ..sized(name, &["gex45"], MAX_5X, Room { five_hour: five_hour_left, week: 90.0, week_reset_h: 100.0 }) }
}

fn placed(accounts: &[Account], eta_h: Option<f64>) -> Result<(String, Option<String>), String> {
    let hosts = [host("gex45", 24, Ok(22))];
    let cloud = Cloud::Closed("no credit".into());
    let capacity = Capacity { here: "gex45", cloud: &cloud, accounts, charge: 1.0 };
    let candidates = [Candidate { host: "gex45".into(), refusal: None }];
    place(&ready(eta_h), &capacity, &candidates, &mut Pools::new(&hosts, accounts)).map(|launch| (launch.account, launch.burn))
}

#[test]
fn a_launch_whose_eta_at_the_accounts_measured_burn_would_cross_its_five_hour_stop_moves_to_an_account_that_fits_and_names_the_burn() {
    let burning = account("claude:b", 10.0, Some(12.0));
    let steady = account("claude:a", 8.0, Some(2.0));
    let (account, burn) = placed(&[burning.clone(), steady.clone()], Some(1.0)).unwrap();
    assert_eq!(account, "claude:a", "b's 12 points over an hour pass its 10 left; a's 2 of 8 do not");
    assert_eq!(burn.unwrap(), "claude:a measured burn 2.0%/h over a 60 min ETA spends 2.0 of the 8 points left to its 5h stop");
    assert_eq!(placed(&[burning.clone(), steady], Some(0.5)).unwrap().0, "claude:b", "half an hour at 12%/h spends 6 of b's 10, and b has the room that ranks higher");
}

#[test]
fn a_launch_no_account_can_finish_before_its_stop_is_held_with_every_burn_named() {
    let held = placed(&[account("claude:b", 10.0, Some(12.0))], Some(1.0)).unwrap_err();
    assert_eq!(held, "gex45 holds the box: claude:b measured burn 12.0%/h over a 60 min ETA spends 12.0 of the 10 points left to its 5h stop");
}

#[test]
fn an_unmeasured_burn_or_an_unknown_eta_never_holds_a_launch() {
    assert!(placed(&[account("claude:b", 10.0, None)], Some(1.0)).is_ok());
    assert_eq!(placed(&[account("claude:b", 10.0, Some(99.0))], None).unwrap().1, None);
}
