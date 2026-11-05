use std::collections::BTreeMap;

use super::detect::{Action, Owner, Settings};
use super::replayed;

const SAMPLES: &str = include_str!("fixtures/overload-2026-10-29.jsonl");
const OWNERS: &str = include_str!("fixtures/overload-2026-10-29.owners.json");
const LAPTOP_CPUS: f64 = 16.0;

fn owners() -> BTreeMap<String, Owner> {
    serde_json::from_str(OWNERS).unwrap()
}

// KEEP(bug): the recorded 2026-10-29 overload is named, capped and reported to owner and coordinators as one incident
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

const GEX45_CPUS: f64 = 20.0;
const FARM_SLICE: &str = "user.slice/user-900.slice";
const FARM_RUNNER: &str = "user.slice/user-900.slice/user@900.service/app.slice/runner-farm-big.service";

fn gex45_split(farm_cores: f64, quota_percent: f64) -> String {
    leased_only(0.0)
        .lines()
        .map(|line| {
            let mut sample: serde_json::Value = serde_json::from_str(line).unwrap();
            let busy = sample["busyCores"].as_f64().unwrap();
            let cgroups = sample["cgroups"].as_array_mut().unwrap();
            cgroups.push(serde_json::json!({ "cgroup": FARM_SLICE, "cores": farm_cores, "quotaPercent": quota_percent }));
            cgroups.push(serde_json::json!({ "cgroup": FARM_RUNNER, "cores": farm_cores }));
            sample["busyCores"] = (busy + farm_cores).into();
            sample["cpus"] = GEX45_CPUS.into();
            sample["load"] = serde_json::json!([60.0, 55.0, 50.0]);
            sample.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// KEEP(bug): an overload with the farm inside its quota sends no page; regression of aa6d7a21d (gex45 paged the proxy four times)
#[test]
fn an_overload_with_the_farm_inside_its_quota_and_leased_work_beside_it_is_recorded_and_sends_no_message() {
    let actions = replayed(&gex45_split(12.0, 1400.0), &mut BTreeMap::new(), Settings::bundled(GEX45_CPUS));
    assert!(actions.iter().any(|action| matches!(action, Action::Incident(_))), "{actions:#?}");
    assert!(sends(&actions).is_empty() && !actions.iter().any(|action| matches!(action, Action::Cap(_))), "{actions:#?}");
}

const BYSTANDER: &str = "agent.slice/north-fleet-wisp-230-d1945-0f1e2d3c.service";

fn gex45_ssh_overload(offender: &str) -> String {
    (0..3)
        .map(|at| {
            serde_json::json!({
                "at": format!("2026-11-06T02:45:{at:02}Z"), "load": [42.26, 40.0, 30.0], "cpuPsi": 80.0, "cpuPsi60": 60.0, "cpus": GEX45_CPUS, "busyCores": 18.08,
                "cgroups": [{ "cgroup": offender, "cores": 17.0 }, { "cgroup": BYSTANDER, "cores": 1.08 }],
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn caps(actions: &[Action]) -> Vec<&str> {
    actions.iter().filter_map(|action| if let Action::Cap(cap) = action { Some(cap.unit.as_str()) } else { None }).collect()
}

// KEEP(bug): on 2026-11-06 gex45 capped the bystander wisp-230 while a laptop session's ssh work ran 17 cores; the session's ssh scope is capped instead
#[test]
fn a_fleet_sessions_ssh_scope_is_capped_ahead_of_a_smaller_fleet_unit() {
    let actions = replayed(&gex45_ssh_overload("app.slice/north-ssh-n-626-repro-0a1b2c3d.scope"), &mut BTreeMap::new(), Settings::bundled(GEX45_CPUS));
    assert_eq!(caps(&actions), ["north-ssh-n-626-repro-0a1b2c3d.scope"], "{actions:#?}");
    assert!(sends(&actions).contains(&"n-626-repro"), "{actions:#?}");
}

// KEEP(bug): work attributed to a session in a scope the watchdog cannot cap must not divert the cap onto a smaller bystander
#[test]
fn a_larger_attributed_offender_the_watchdog_cannot_cap_leaves_the_smaller_fleet_unit_uncapped() {
    let mut owners = BTreeMap::from([("session-61845.scope".to_owned(), Owner { session: "n-626-repro".to_owned(), parent: None })]);
    let actions = replayed(&gex45_ssh_overload("user.slice/user-1000.slice/session-61845.scope"), &mut owners, Settings::bundled(GEX45_CPUS));
    assert!(caps(&actions).is_empty(), "{actions:#?}");
    assert!(sends(&actions).contains(&"n-626-repro"), "{actions:#?}");
}
