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
