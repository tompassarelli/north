use serde_json::json;

use super::guards::actions_verdict;
use super::measure::{Env, measure};
use super::render::format_row;
use super::tests::{Fake, places};

const NOW: f64 = 1_793_300_000.0;
const MAIN_TREE: &str = "1111111111111111111111111111111111111111";
const HOUR: f64 = 3600.0;

fn sha(index: usize) -> String {
    format!("{index:040x}")
}

fn lane_listing(ages_hours: &[f64]) -> String {
    ages_hours.iter().enumerate().map(|(index, age)| format!("{} {} land-{index}\n", sha(index), (NOW - age * HOUR) as i64)).collect()
}

fn measured(ages_hours: &[f64], landed: usize) -> serde_json::Value {
    measured_with(ages_hours, landed, "local", 0)
}

fn measured_with(ages_hours: &[f64], landed: usize, queue: &str, tickets: usize) -> serde_json::Value {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let origin = "https://github.com/tompassarelli/makgora.git";
    let waiting = crate::platform::land_queue::directory(&places.state, origin);
    std::fs::create_dir_all(&waiting).unwrap();
    for ticket in 0..tickets {
        std::fs::write(waiting.join(format!("{}-{ticket}.json", (NOW as u64 - 60) * 1_000_000)), "{}").unwrap();
    }
    let listing = lane_listing(ages_hours);
    let shas: String = (0..ages_hours.len()).map(|index| format!("{}\n", sha(index))).collect();
    let reply = |argv: &[&str]| match argv {
        ["git", "remote", "get-url", "origin"] | ["git", "remote", "get-url", "--push", "origin"] => Some(format!("{origin}\n")),
        ["git", "config", ..] => Some(format!("{queue}\n")),
        ["gh", "run", "list", ..] => Some("[]".into()),
        ["git", "fetch", ..] => Some(String::new()),
        ["git", "log", ..] => Some(String::new()),
        ["git", "for-each-ref", "--no-merged=origin/main", ..] => Some(shas.clone()),
        ["git", "for-each-ref", ..] => Some(listing.clone()),
        ["git", "rev-parse", "origin/main^{tree}"] => Some(format!("{MAIN_TREE}\n")),
        ["git", "merge-tree", "--write-tree", "origin/main", lane] => {
            let index = (0..ages_hours.len()).find(|index| sha(*index) == *lane).unwrap();
            Some(format!("{}\n", if index < landed { MAIN_TREE.to_owned() } else { "2222222222222222222222222222222222222222".into() }))
        }
        _ => None,
    };
    let host = Fake(&reply);
    serde_json::Value::Object(measure(&Env::new(&places, &host, Default::default()), "makgora", NOW))
}

#[test]
fn test_nine_stale_lanes_are_cleanup_not_a_stall() {
    let stale = [17.0, 18.0, 20.0, 26.0, 30.0, 40.0, 50.0, 60.0, 72.0];
    let row = measured(&stale, 3);
    assert_eq!(row["pending"], 0);
    assert_eq!(row["cleanup"].as_array().unwrap().len(), 9);
    assert!(format_row(&row, None).contains("cleanup 9 lanes (not a stall)"), "{}", format_row(&row, None));
}

#[test]
fn test_a_fresh_unlanded_lane_still_counts_beside_the_cleanup() {
    let mut ages = vec![0.5, 17.0, 18.0, 26.0];
    let row = measured(&ages, 0);
    assert_eq!(row["pending"], 3);
    assert_eq!(row["cleanup"], json!(["land-3"]));
    ages.truncate(1);
    assert_eq!(measured(&ages, 0)["pending"], 1);
}

fn stranded_listing() -> String {
    let session = |name: &str, lane: &str, minutes_ago: f64, ahead: u32| {
        json!({"name": name, "provider": "claude", "account": "a", "status": "exited", "repo": "makgora", "tasks": [], "machine": "gex45", "stop": "capacity", "ended_ms": ((NOW - minutes_ago * 60.0) * 1000.0) as u64,
            "lanes": [{"path": format!("/home/tom/code/makgora/worktrees/{lane}"), "branch": lane, "ahead": ahead, "dirty": false, "landed": false}]})
    };
    json!([session("sc-b1", "sc-1-lane", 190.0, 2), session("sc-b2", "sc-2-lane", 5.0, 1), session("sc-b3", "sc-3-lane", 300.0, 0)]).to_string()
}

#[test]
fn test_a_lane_stranded_over_fifteen_minutes_is_listed_with_its_stop_and_alerts() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let listing = stranded_listing();
    let reply = |argv: &[&str]| match argv {
        ["north", "fleet", "stranded", "--json"] => Some(listing.clone()),
        _ => None,
    };
    let host = Fake(&reply);
    let row = serde_json::Value::Object(measure(&Env::new(&places, &host, Default::default()), "makgora", NOW));
    let [lane] = row["stranded"].as_array().unwrap().as_slice() else { panic!("{row}") };
    assert_eq!((lane["session"].as_str(), lane["lane"].as_str(), lane["stop"].as_str()), (Some("sc-b1"), Some("sc-1-lane"), Some("Stop: capacity")), "a five-minute stop and a lane with nothing unlanded are not listed");
    assert!(format_row(&row, None).contains("stranded 1 lanes: sc-b1 sc-1-lane (Stop: capacity, 190m)"), "{}", format_row(&row, None));
    let nodes = [super::alerts::Node { id: "makgora-parent".into(), parent: None, active: true, cwd: "/home/tom/code/makgora/main".into() }];
    let (state, actions) = super::tests::second_run(std::slice::from_ref(&row), NOW, &nodes);
    assert!(state.contains_key("makgora"));
    let texts: Vec<&str> = actions.iter().map(|action| action.text.as_str()).collect();
    assert_eq!(texts, ["Lanes stranded in makgora, unlanded with no live session: sc-b1 sc-1-lane (Stop: capacity, 190m)"; 2], "the parent and the proxy");
}

#[test]
fn test_stale_lane_branches_and_a_land_queue_are_not_idle_actions_work() {
    let stale = [0.5, 1.0, 2.0, 3.0, 5.0, 8.0, 10.0, 12.0, 17.0, 26.0, 40.0, 72.0];
    let row = measured_with(&stale, 0, "local", 8);
    assert_eq!(row["lander"]["depth"], 8);
    assert!(row["pending"].as_u64().unwrap() >= 8);
    assert_eq!(row["actions_pending"], 0);
    let now = NOW;
    let quiet: Vec<(f64, u64)> = (0..5).map(|i| (now - 900.0 - 60.0 + 300.0 * f64::from(i), 1)).collect();
    let verdict = actions_verdict(row["actions_pending"].as_u64(), &quiet, now);
    assert_eq!(verdict.reason, None);
    let autoland = measured_with(&stale, 0, "autoland", 0);
    assert_eq!(autoland["actions_pending"], 9, "under Autoland the live lanes are Actions work and the one-day-old ones are not");
    assert!(actions_verdict(autoland["actions_pending"].as_u64(), &quiet, now).reason.is_some());
}
