use serde_json::json;

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
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let listing = lane_listing(ages_hours);
    let shas: String = (0..ages_hours.len()).map(|index| format!("{}\n", sha(index))).collect();
    let reply = |argv: &[&str]| match argv {
        ["git", "remote", "get-url", "origin"] => Some("https://github.com/tompassarelli/smashcraft.git\n".into()),
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
    serde_json::Value::Object(measure(&Env::new(&places, &host, Default::default()), "smashcraft", NOW))
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
        json!({"name": name, "provider": "claude", "account": "a", "status": "exited", "repo": "smashcraft", "tasks": [], "machine": "gex45", "stop": "capacity", "ended_ms": ((NOW - minutes_ago * 60.0) * 1000.0) as u64,
            "lanes": [{"path": format!("/home/tom/code/smashcraft/worktrees/{lane}"), "branch": lane, "ahead": ahead, "dirty": false, "landed": false}]})
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
    let row = serde_json::Value::Object(measure(&Env::new(&places, &host, Default::default()), "smashcraft", NOW));
    let [lane] = row["stranded"].as_array().unwrap().as_slice() else { panic!("{row}") };
    assert_eq!((lane["session"].as_str(), lane["lane"].as_str(), lane["stop"].as_str()), (Some("sc-b1"), Some("sc-1-lane"), Some("Stop: capacity")), "a five-minute stop and a lane with nothing unlanded are not listed");
    assert!(format_row(&row, None).contains("stranded 1 lanes: sc-b1 sc-1-lane (Stop: capacity, 190m)"), "{}", format_row(&row, None));
    let nodes = [super::alerts::Node { id: "smashcraft-parent".into(), parent: None, active: true, cwd: "/home/tom/code/smashcraft/main".into() }];
    let (state, actions) = super::alerts::alert_plan(std::slice::from_ref(&row), &Default::default(), NOW, &nodes);
    assert!(state.contains_key("smashcraft"));
    let texts: Vec<&str> = actions.iter().map(|action| action.text.as_str()).collect();
    assert_eq!(texts, ["Lanes stranded in smashcraft, unlanded with no live session: sc-b1 sc-1-lane (Stop: capacity, 190m)"; 2], "the parent and the proxy");
}
