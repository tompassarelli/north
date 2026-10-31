use std::collections::BTreeMap;

use super::detect::{Action, Owner, Settings};
use super::replayed;

const SAMPLES: &str = include_str!("fixtures/overload-2026-10-29.jsonl");
const OWNERS: &str = include_str!("fixtures/overload-2026-10-29.owners.json");
const LAPTOP_CPUS: f64 = 16.0;

fn owners() -> BTreeMap<String, Owner> {
    serde_json::from_str(OWNERS).unwrap()
}

#[test]
fn replaying_the_2026_10_29_overload_names_the_owner_caps_its_scope_and_wakes_owner_and_coordinators() {
    let actions = replayed(SAMPLES, &mut owners(), Settings::bundled(LAPTOP_CPUS));
    let incidents: Vec<_> = actions.iter().filter_map(|action| if let Action::Incident(incident) = action { Some(incident) } else { None }).collect();
    assert_eq!(incidents.len(), 1, "one incident for the whole episode: {actions:#?}");
    let incident = incidents[0];
    assert!(incident.psi60 >= 15.0 && incident.load_per_core >= 2.0, "{incident:?}");
    let top = incident.trees.iter().find(|tree| tree.session.is_some()).expect("a tree with a fleet owner");
    assert_eq!(top.session.as_deref(), Some("n-333-revive"));
    assert!(top.unit.starts_with("north-session-77c49ab1"));
    let caps: Vec<_> = actions.iter().filter_map(|action| if let Action::Cap(cap) = action { Some(cap) } else { None }).collect();
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0].unit, "north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope");
    assert!(caps[0].cpu_quota_percent >= 100 && caps[0].cpu_weight < 100);
    let mut recipients: Vec<_> = actions.iter().filter_map(|action| if let Action::Send { to, .. } = action { Some(to.as_str()) } else { None }).collect();
    recipients.sort_unstable();
    assert_eq!(recipients, ["landing-blockers", "n-333-revive", "proxy"]);
    let text = actions.iter().find_map(|action| if let Action::Send { text, .. } = action { Some(text) } else { None }).unwrap();
    assert!(text.contains("session n-333-revive") && text.contains("Capped north-session-77c49ab1"), "{text}");
}

fn calmed(line: &str, psi60: f64, at: Option<&str>) -> String {
    let mut sample: serde_json::Value = serde_json::from_str(line).unwrap();
    sample["cpuPsi60"] = psi60.into();
    sample["load"] = serde_json::json!([1.0, 1.0, 1.0]);
    if let Some(at) = at {
        sample["at"] = at.into();
    }
    sample.to_string()
}

#[test]
fn a_calm_host_makes_no_incident() {
    let calm: Vec<_> = SAMPLES.lines().map(|line| calmed(line, 0.0, None)).collect();
    assert!(replayed(&calm.join("\n"), &mut owners(), Settings::bundled(LAPTOP_CPUS)).is_empty());
}

#[test]
fn caps_lift_after_load_stays_calm() {
    let hot = SAMPLES.lines().take(6).map(str::to_owned);
    let calm = (0..3).map(|n| calmed(SAMPLES.lines().next().unwrap(), 1.0, Some(&format!("2026-10-29T15:3{n}:00Z"))));
    let actions = replayed(&hot.chain(calm).collect::<Vec<_>>().join("\n"), &mut owners(), Settings::bundled(LAPTOP_CPUS));
    assert!(actions.iter().any(|action| matches!(action, Action::Release(cap) if cap.unit.starts_with("north-session-77c49ab1"))), "{actions:#?}");
}

#[test]
fn a_top_tree_in_a_north_fleet_service_unit_names_its_session_and_sends_to_it() {
    let unit = "north-fleet-sc-butcher-hook-a5f3b12f.service";
    let hot: Vec<_> = SAMPLES
        .lines()
        .map(|line| {
            let mut sample: serde_json::Value = serde_json::from_str(line).unwrap();
            sample["cgroups"].as_array_mut().unwrap().push(serde_json::json!({ "cgroup": format!("app.slice/{unit}"), "cores": 40.0 }));
            sample.to_string()
        })
        .collect();
    let actions = replayed(&hot.join("\n"), &mut BTreeMap::new(), Settings::bundled(LAPTOP_CPUS));
    let incident = actions.iter().find_map(|action| if let Action::Incident(incident) = action { Some(incident) } else { None }).expect("an incident");
    let tree = incident.trees.iter().find(|tree| tree.unit == unit).expect("the service unit is a named tree");
    assert_eq!(tree.session.as_deref(), Some("sc-butcher-hook"));
    assert!(actions.iter().any(|action| matches!(action, Action::Send { to, text } if to == "sc-butcher-hook" && text.contains("session sc-butcher-hook"))), "{actions:#?}");
}

fn with_cgroup(cgroup: &str, cores: f64) -> String {
    SAMPLES
        .lines()
        .map(|line| {
            let mut sample: serde_json::Value = serde_json::from_str(line).unwrap();
            sample["cgroups"].as_array_mut().unwrap().push(serde_json::json!({ "cgroup": cgroup, "cores": cores }));
            sample.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn leased_only(unattributed: f64) -> String {
    SAMPLES
        .lines()
        .map(|line| {
            let mut sample: serde_json::Value = serde_json::from_str(line).unwrap();
            let kept: Vec<_> = sample["cgroups"].as_array().unwrap().iter().filter(|entry| entry["cgroup"].as_str().is_some_and(|cgroup| cgroup.contains("agent-capacity-") || !(cgroup.ends_with(".scope") || cgroup.ends_with(".service")))).cloned().collect();
            let leased: f64 = kept.iter().filter(|entry| entry["cgroup"].as_str().unwrap().ends_with(".scope")).map(|entry| entry["cores"].as_f64().unwrap()).sum();
            sample["cgroups"] = kept.into();
            sample["busyCores"] = (leased + unattributed).into();
            sample.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sends(actions: &[Action]) -> Vec<&str> {
    let mut to: Vec<_> = actions.iter().filter_map(|action| if let Action::Send { to, .. } = action { Some(to.as_str()) } else { None }).collect();
    to.sort_unstable();
    to
}

#[test]
fn an_unleased_top_tree_in_a_fleet_service_unit_is_capped_and_told_to_its_owner_and_parent() {
    let unit = "north-fleet-sc-butcher-hook-a5f3b12f.service";
    let mut owners = BTreeMap::from([("north-session-1.scope".to_owned(), Owner { session: "sc-butcher-hook".into(), parent: Some("sc-lead".into()) })]);
    let actions = replayed(&with_cgroup(&format!("app.slice/{unit}"), 40.0), &mut owners, Settings::bundled(LAPTOP_CPUS));
    let cap = actions.iter().find_map(|action| if let Action::Cap(cap) = action { Some(cap) } else { None }).expect("the service unit is capped");
    assert_eq!((cap.unit.as_str(), cap.session.as_deref()), (unit, Some("sc-butcher-hook")));
    assert_eq!(sends(&actions), ["proxy", "sc-butcher-hook", "sc-lead"]);
}

#[test]
fn load_in_an_ssh_session_scope_belongs_to_the_fleet_session_whose_ssh_client_started_it_and_is_unleased() {
    let mut owners = BTreeMap::from([("session-4242.scope".to_owned(), Owner { session: "sc-extract".into(), parent: None })]);
    let actions = replayed(&with_cgroup("user.slice/user-1000.slice/session-4242.scope", 4.0), &mut owners, Settings::bundled(LAPTOP_CPUS));
    let incident = actions.iter().find_map(|action| if let Action::Incident(incident) = action { Some(incident) } else { None }).expect("an incident");
    let tree = incident.trees.iter().find(|tree| tree.unit == "session-4242.scope").expect("the ssh scope is a ranked tree");
    assert_eq!((tree.kind, tree.session.as_deref()), ("ssh", Some("sc-extract")));
    assert!(sends(&actions).contains(&"sc-extract"), "{actions:#?}");
}

#[test]
fn an_overload_with_every_top_tree_leased_and_all_load_attributed_is_recorded_and_sends_no_message() {
    let actions = replayed(&leased_only(0.0), &mut BTreeMap::new(), Settings::bundled(LAPTOP_CPUS));
    assert!(actions.iter().any(|action| matches!(action, Action::Incident(_))), "{actions:#?}");
    assert!(sends(&actions).is_empty() && !actions.iter().any(|action| matches!(action, Action::Cap(_))), "{actions:#?}");
    let hidden = replayed(&leased_only(6.0), &mut BTreeMap::new(), Settings::bundled(LAPTOP_CPUS));
    assert_eq!(sends(&hidden), ["proxy"], "{hidden:#?}");
}
