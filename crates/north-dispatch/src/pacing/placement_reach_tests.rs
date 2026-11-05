use super::placement::gate;
use super::placement_gate_tests::{control, decide, gate as quota, target};
use toml_edit::DocumentMut;

const NEXUS_BOX: &str = "Item: north#12 read the nexus job queue\nSurface: rust\nNeeds: nexus\n";
const CONFIG: &str = "[hosts.whiterabbit]\nreaches = [\"gex45\"]\n\
    [hosts.gex45]\nssh = \"gex45\"\nresident_cap = 4\nreaches = [\"whiterabbit\"]\n\
    [hosts.nexus]\nresident_cap = 4\n\
    [hosts.relay]\nssh = \"relay\"\nresident_cap = 4\nreaches = [\"nexus\"]\n";

fn place(text: &str, config: &str) -> (String, Vec<String>) {
    let cfg: DocumentMut = config.parse().unwrap();
    let mut machines = gate::machines(&cfg);
    for machine in &mut machines {
        machine.live = Some(0);
        machine.accounts = vec!["a".into()];
    }
    let usage = quota(&[("claude:a", control((10.0, 1.0, "2026-10-19T04:00:00Z"), (10.0, 1.0)))]);
    let decision = decide(text, &cfg, &usage, (false, String::new()), &machines, "whiterabbit");
    (format!("{} on {}", target(&decision), decision.machine.map_or("here".to_owned(), |machine| machine.name)), decision.machine_lines)
}

#[test]
fn a_box_that_needs_a_host_no_machine_reaches_is_refused_naming_the_host() {
    let without_relay = CONFIG.split("[hosts.relay]").next().unwrap();
    let (placed, lines) = place(NEXUS_BOX, without_relay);
    assert_eq!(placed, "unserved: no host reaches nexus on here");
    assert!(lines.contains(&"gex45 skipped: cannot reach nexus".to_owned()), "{lines:?}");
}

#[test]
fn a_box_that_needs_a_host_a_machine_reaches_is_placed_on_that_machine() {
    let (placed, lines) = place(NEXUS_BOX, CONFIG);
    assert_eq!(placed, "claude:a on relay", "{lines:?}");
    assert!(lines.contains(&"gex45 skipped: cannot reach nexus".to_owned()), "{lines:?}");
}

#[test]
fn a_host_that_declares_no_reach_list_reaches_every_host_and_a_box_may_need_the_host_it_runs_on() {
    let open = "[hosts.gex45]\nssh = \"gex45\"\nresident_cap = 4\n[hosts.nexus]\nresident_cap = 4\n";
    assert_eq!(place(NEXUS_BOX, open).0, "claude:a on gex45");
    let own = "Item: north#12 read gex45 logs\nSurface: rust\nNeeds: gex45\n";
    assert_eq!(place(own, CONFIG.split("[hosts.nexus]").next().unwrap()).0, "claude:a on gex45");
}
