use super::plan::{self, Capacity, Cloud, Ready};
use super::slots::{Account, Pools, Room};
use super::survey::{self, Sample};
use super::tests::{CONFIG, MAX_5X, accounts_of, chosen, host, sized};
use super::tier::{Route, Tier};
use crate::pacing::placement::{gate, room::Basis};
use serde_json::json;

const AT: f64 = 1_793_932_153.97;

fn placed_on_claude_a(working: i64) -> (Vec<plan::Launch>, Vec<String>, Account) {
    let control = json!({"allowed_workers": 1, "held": true, "over": true, "at": AT});
    let usage = json!({"windows": [
        {"window": "5h", "used_pct": 0.0, "burn_pct_h": 0.0, "reset_h": 0.0, "resets_at": "2026-10-31T04:00:00Z"},
        {"window": "week", "used_pct": 82.0, "burn_pct_h": 1.967, "reset_h": 143.5, "resets_at": "2026-11-12T02:00:00Z"}
    ]});
    let cfg = CONFIG.parse().unwrap();
    let basis = Basis::new(&cfg, json!({}), AT);
    let sample = Sample { name: "claude:a", control: &control, usage: Some(&usage), working };
    let account = survey::account(&sample, &basis, vec!["gex45".into()], AT, &mut Vec::new());
    let accounts = vec![account.clone()];
    let mut pools = Pools::new(&[host("gex45", 24, Ok(0))], &accounts);
    let cloud = Cloud::Closed("no credit".into());
    let capacity = Capacity { here: "whiterabbit", cloud: &cloud, accounts: &accounts, charge: Room::box_reserve() };
    let route = Route { tier: Tier::Opus, category: "feature".into(), arm: false, effort: None, effort_arm: false, follows: None, why: String::new() };
    let candidates = [plan::Candidate { host: "gex45".into(), refusal: None }];
    let (mut placed, mut refused) = (Vec::new(), Vec::new());
    for number in 0..12 {
        let ready = Ready { item: super::issues::IssueRef::new("north", 600 + number), brief: String::new(), code_only: false, route: route.clone(), eta_h: Some(99.0 / 60.0) };
        match plan::place(&ready, &capacity, &candidates, &mut pools) {
            Ok(launch) => placed.push(launch),
            Err(why) => refused.push(why),
        }
    }
    (placed, refused, account)
}

#[test]
fn the_week_bounds_launches_by_its_last_point_and_by_pace_holding_the_2026_11_06_reading_of_claude_a() {
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

    let (placed, refused, held) = placed_on_claude_a(4);
    assert_eq!(held.burn, None, "a 5h window whose reset passed measures no burn");
    assert_eq!(held.workers, Some(0));
    assert!(placed.is_empty(), "{:?}", placed.iter().map(|launch| &launch.item).collect::<Vec<_>>());
    assert!(refused[0].contains("claude:a is held by pace with no session left to start"), "{}", refused[0]);
    let (placed, refused, _) = placed_on_claude_a(0);
    assert_eq!((placed.len(), refused.len()), (1, 11), "pace's allowed_workers 1 with none working leaves one launch");
    let burn = placed[0].burn.as_deref().unwrap();
    assert!(burn.contains("week 2.0%/h spends 3.2 of the 18 points left in its week") && !burn.contains("5h"), "{burn}");
}
