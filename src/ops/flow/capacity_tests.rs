use std::cell::RefCell;

use serde_json::json;

use super::alerts::State;
use super::capacity::{Reading, parse, remote_hosts, saturation, saturation_alert, saturation_plan};
use super::tests::{Fake, places};

const NOW: f64 = 1_793_300_000.0;
const FLOOR: f64 = 15.0;

fn reading(queued: u64, disk_free_gib: f64) -> Reading {
    Reading { host: "gex45".into(), queued, disk_free_gib }
}

#[test]
fn test_gex45_incident_readings_are_saturated() {
    assert!(saturation(&reading(9, 14.0), FLOOR).unwrap().contains("9 jobs queued"));
    assert!(saturation(&reading(0, 14.0), FLOOR).unwrap().contains("14 GiB"));
    assert!(saturation(&reading(4, 300.0), FLOOR).is_some());
    assert_eq!(saturation(&reading(3, 15.0), FLOOR), None);
}

#[test]
fn test_probe_output_is_parsed_with_queue_as_list_or_count() {
    assert_eq!(parse("gex45", "{\"queued\":[{},{},{}]}\n---\n 42G\n"), Some(reading(3, 42.0)));
    assert_eq!(parse("gex45", "{\"queued\":5}\n---\n7G\n"), Some(reading(5, 7.0)));
    assert_eq!(parse("gex45", "not json"), None);
}

#[test]
fn test_ssh_hosts_come_from_orchestration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("orchestration.toml");
    std::fs::write(&path, "[hosts.nexus]\nresident_cap = 6\n[hosts.gex45]\nssh = \"tom@gex45\"\n").unwrap();
    assert_eq!(remote_hosts(&path), [("gex45".to_owned(), "tom@gex45".to_owned())]);
}

#[test]
fn test_saturation_waits_ten_minutes_then_repeats_every_fifteen_and_clears() {
    let (mut state, mut sent) = (State::new(), Vec::new());
    for minute in 0..=40 {
        let now = NOW + f64::from(minute) * 60.0;
        let (next, actions) = saturation_plan(&[reading(9, 12.0)], &state, now, FLOOR);
        state = next;
        for action in actions {
            state.get_mut(&action.repo).unwrap().sent.insert(action.key.clone(), now);
            assert_eq!((action.node.as_str(), action.channel), ("proxy", "fleet"));
            sent.push(minute);
        }
    }
    assert_eq!(sent, [10, 24, 38]);
    let (cleared, actions) = saturation_plan(&[reading(1, 90.0)], &state, NOW + 3000.0, FLOOR);
    assert!(cleared.is_empty() && actions.is_empty());
    let (silent, _) = saturation_plan(&[], &state, NOW + 3000.0, FLOOR);
    assert_eq!(silent, state);
}

#[test]
fn test_saturation_alert_reaches_the_proxy_through_fleet_send() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let policy = directory.path().join("orchestration.toml");
    std::fs::write(&policy, "[hosts.gex45]\nssh = \"tom@gex45\"\n").unwrap();
    let sends = RefCell::new(Vec::<String>::new());
    let fleet = json!([{"name": "proxy", "conversation": "proxy-session", "parent": null, "provider": "claude", "status": "idle", "cwd": "/home/tom"}]).to_string();
    let reply = |argv: &[&str]| match argv {
        ["north", "fleet", "list", "--json"] => Some(fleet.clone()),
        ["north", "fleet", "send", node, text] => {
            sends.borrow_mut().push(format!("{node}: {text}"));
            Some(String::new())
        }
        ["hostname"] => Some("whiterabbit\n".into()),
        ["sh", "-c", _] => Some("{\"queued\":[]}\n---\n200G\n".into()),
        ["ssh", .., "tom@gex45", _] => Some("{\"queued\":[1,2,3,4,5,6,7,8,9]}\n---\n12G\n".into()),
        _ => None,
    };
    let host = Fake(&reply);
    saturation_alert(&places, &host, &policy, NOW).unwrap();
    assert!(sends.borrow().is_empty());
    saturation_alert(&places, &host, &policy, NOW + 600.0).unwrap();
    saturation_alert(&places, &host, &policy, NOW + 900.0).unwrap();
    assert_eq!(sends.borrow().len(), 1);
    assert!(sends.borrow()[0].starts_with("proxy: Host gex45 saturated for 10m: 9 jobs queued"), "{}", sends.borrow()[0]);
}
