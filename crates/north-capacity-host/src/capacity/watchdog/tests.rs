use std::collections::BTreeMap;

use super::detect::{Action, Owner, Settings};
use super::replayed;

fn sends(actions: &[Action]) -> Vec<&str> {
    let mut to: Vec<_> = actions.iter().filter_map(|action| if let Action::Send { to, .. } = action { Some(to.as_str()) } else { None }).collect();
    to.sort_unstable();
    to
}

const GEX45_CPUS: f64 = 20.0;

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

#[test]
fn a_fleet_sessions_ssh_scope_is_capped_ahead_of_a_smaller_fleet_unit_unless_a_larger_offender_cannot_be_capped() {
    let actions = replayed(&gex45_ssh_overload("app.slice/north-ssh-n-626-repro-0a1b2c3d.scope"), &mut BTreeMap::new(), Settings::bundled(GEX45_CPUS));
    assert_eq!(caps(&actions), ["north-ssh-n-626-repro-0a1b2c3d.scope"], "{actions:#?}");
    assert!(sends(&actions).contains(&"n-626-repro"), "{actions:#?}");
    let mut owners = BTreeMap::from([("session-61845.scope".to_owned(), Owner { session: "n-626-repro".to_owned(), parent: None })]);
    let actions = replayed(&gex45_ssh_overload("user.slice/user-1000.slice/session-61845.scope"), &mut owners, Settings::bundled(GEX45_CPUS));
    assert!(caps(&actions).is_empty(), "{actions:#?}");
    assert!(sends(&actions).contains(&"n-626-repro"), "{actions:#?}");
}
