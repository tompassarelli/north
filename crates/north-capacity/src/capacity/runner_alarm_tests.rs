#![cfg(test)]

use super::runner_alarm::{HostReading, LeaseUse, State, step};

const MINUTE: i64 = 60_000;

fn grid(cpx62: usize, cpx62b: usize, gex45: usize, gex45_checks: &[f64]) -> Vec<HostReading> {
    let checks = gex45_checks.iter().enumerate().map(|(index, cores)| LeaseUse { lease: format!("lease-{index}"), cores: Some(*cores) }).collect();
    vec![
        HostReading { host: "cpx62".into(), runners: Some(cpx62), land_checks: Vec::new() },
        HostReading { host: "cpx62b".into(), runners: Some(cpx62b), land_checks: Vec::new() },
        HostReading { host: "gex45".into(), runners: Some(gex45), land_checks: checks },
    ]
}

fn pages_over(states: &[Vec<HostReading>]) -> Vec<String> {
    let mut state = State::default();
    let mut pages = Vec::new();
    for (at, hosts) in (0..).map(|minute: i64| minute * MINUTE).zip(states) {
        let (next, sent) = step(state, at, hosts);
        state = next;
        pages.extend(sent);
    }
    pages
}

#[test]
fn the_runner_alarm_pages_once_per_episode_when_the_grid_loses_its_runners_or_its_land_checks_idle() {
    let busy = grid(12, 12, 4, &[3.5, 2.0, 1.2, 0.8, 4.0]);
    assert!(pages_over(&vec![busy.clone(); 20]).is_empty());

    assert!(pages_over(&vec![grid(0, 12, 4, &[]); 10]).is_empty(), "one cpx host closed for an eval leaves 16 runners, the floor");

    let incident: Vec<Vec<HostReading>> = [vec![busy.clone(); 2], vec![grid(0, 0, 4, &[]); 10], vec![busy.clone(); 2], vec![grid(0, 0, 4, &[]); 3]].concat();
    let pages = pages_over(&incident);
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages.iter().all(|page| page.contains("4 live remote runners") && page.contains("cpx62 0, cpx62b 0, gex45 4")), "{pages:?}");

    let stalled = grid(12, 12, 4, &[0.02, 0.0, 0.05, 2.5, 3.0]);
    let idle: Vec<Vec<HostReading>> = [vec![busy.clone(); 2], vec![stalled.clone(); 12], vec![busy.clone(); 2], vec![stalled; 7]].concat();
    let pages = pages_over(&idle);
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages.iter().all(|page| page.contains("3 of 5 land-check leases on gex45")), "{pages:?}");

    let short = [vec![grid(12, 12, 4, &[0.02, 0.0, 0.05, 2.5, 3.0]); 4], vec![busy; 4]].concat();
    assert!(pages_over(&short).is_empty(), "idle under 5 minutes pages nothing");
}
