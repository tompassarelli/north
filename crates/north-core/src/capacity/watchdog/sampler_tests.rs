use std::process::Command;
use std::time::{Duration, Instant};

use super::sampler::{entry, terminate};
use super::sampling::{Incident, Kind, Term};

fn start_of(pid: u32) -> String {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat[stat.rfind(')').unwrap() + 2..].split(' ').nth(19).unwrap().to_owned()
}

#[test]
fn termination_signals_only_a_process_whose_start_time_still_matches() {
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid = i64::from(child.id());
    terminate(&[Term { pid, start: "0".into() }]);
    std::thread::sleep(Duration::from_millis(200));
    assert!(child.try_wait().unwrap().is_none(), "a recycled pid with another start time is left alone");
    terminate(&[Term { pid, start: start_of(child.id()) }]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the matching process was not terminated");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_incident_is_logged_with_its_cgroup_cwd_pids_and_parent_chain() {
    let incident = Incident {
        at: "2026-11-05T12:00:00.000Z".into(),
        kind: Kind::GitMaintenanceTerminated,
        summary: "SIGTERM git repack outside the nightly timer (1 processes) in /srv/repo".into(),
        cgroup: "app.slice/x.scope".into(),
        cores: None,
        seconds: None,
        cwd: Some("/srv/repo".into()),
        chain: vec!["7 git repack".into(), "1 claude".into()],
        pids: vec![7],
    };
    assert_eq!(
        entry(&incident),
        "- 2026-11-05T12:00:00.000Z git-maintenance-terminated: SIGTERM git repack outside the nightly timer (1 processes) in /srv/repo\n  cgroup: app.slice/x.scope\n  cwd: /srv/repo\n  pids: 7\n  < 7 git repack\n  < 1 claude\n"
    );
}
