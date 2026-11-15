#![cfg(test)]

use std::fs;
use std::path::Path;

use super::agent_load::Host;
use super::game_reserve_tests::settings;
use super::mode::{self, Limits, Mode};
use super::mode_tick::{self, Power};

const BUILD_HOST_QUOTA: &str = "systemctl --user set-property --runtime agent.slice CPUQuota=2200%";
const PLAY_QUOTA: &str = "systemctl --user set-property --runtime agent.slice CPUQuota=1200%";

fn host(root: &Path) -> Host {
    Host { proc: root.join("proc"), cgroup: root.join("cgroup"), capacity: root.join("capacity"), game_reserve: root.join("game-reserve.json"), systemctl: root.join("systemctl").display().to_string(), uid: 1000 }
}

fn tick(root: &Path, idle: Option<bool>, now: u64) -> Vec<String> {
    tick_with(root, idle, None, now)
}

fn tick_with(root: &Path, idle: Option<bool>, battery: Option<u8>, now: u64) -> Vec<String> {
    mode_tick::tick_in(&host(root), idle, Power { battery, cpu_celsius: None }, settings(root), &root.join("mode"), now);
    fs::read_to_string(root.join("calls.log")).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn agent_slice_quota_is_the_12_cpu_bound_at_the_desk_and_22_only_while_away() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("mode");
    let limits = Limits::bundled();
    let quotas = |calls: &[String]| calls.iter().filter(|call| call.contains("CPUQuota=")).cloned().collect::<Vec<_>>();
    assert_eq!(quotas(&tick(root.path(), Some(true), 1_000)), [BUILD_HOST_QUOTA]);
    let away = mode::read(&dir);
    assert!(mode::builds_here_at(&away, &limits, 1_010), "away takes land checks");
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_030)), [PLAY_QUOTA], "sitting down at the desk caps agents at 12 CPUs");
    let desk = mode::read(&dir);
    assert_eq!(desk.mode, Mode::Desktop);
    assert!(!mode::builds_here_at(&desk, &limits, 1_040), "the desk takes no heavy land checks");
    assert!(!mode::leases_own_builds_at(false, &desk, &limits, 1_040));
    mode::write(&dir, &mode::hold(&desk, 1_050, "playtest", 30, &limits)).unwrap();
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_060)), Vec::<String>::new(), "a playtest hold keeps the same bound");
    assert!(!mode::builds_here_at(&mode::read(&dir), &limits, 1_070));
    mode::write(&dir, &mode::release(&mode::read(&dir), 1_080)).unwrap();
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_090)), Vec::<String>::new(), "releasing the hold at the desk stays at 12");
    assert_eq!(quotas(&tick(root.path(), Some(true), 1_120)), [BUILD_HOST_QUOTA], "leaving the desk returns 22");

    let home = std::env::var("HOME").unwrap();
    let pressure = |some: f64| fs::write(root.path().join("cgroup/app.slice/io.pressure"), format!("some avg10={some:.2} avg60=0.00 avg300=0.00 total=1\nfull avg10={some:.2} avg60=0.00 avg300=0.00 total=1\n")).unwrap();
    let limits_touched = |calls: Vec<String>| calls.into_iter().filter(|call| call.contains("Bandwidth") || call.contains("CPUQuota=")).collect::<Vec<_>>();
    pressure(51.49);
    let relief = format!("systemctl --user set-property --runtime agent.slice IOReadBandwidthMax={home} 52428800 IOWriteBandwidthMax={home} 52428800");
    assert_eq!(limits_touched(tick(root.path(), Some(true), 1_150)), [relief], "app.slice disk pressure over the seat limit caps agent disk and leaves the CPU quota alone");
    pressure(0.0);
    assert_eq!(limits_touched(tick(root.path(), Some(true), 1_180)), [format!("systemctl --user set-property --runtime agent.slice IOReadBandwidthMax={home} 209715200 IOWriteBandwidthMax={home} 209715200")], "ending relief restores the standing read cap, since a bare reset overrides the read cap agent.slice carries from boot");
}
