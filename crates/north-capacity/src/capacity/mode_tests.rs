#![cfg(test)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::agent_load::{Host, Lease, Observed, plan};
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
}

fn heavy(id: &str, owner: &str) -> Lease {
    Lease { id: id.to_owned(), kind: "run".to_owned(), class: "heavy".to_owned(), aggregate_slice: None, wrapper_pid: None, wrapper_start: Some("1".to_owned()), owner: owner.to_owned() }
}

#[test]
fn on_battery_the_landers_own_checks_keep_running_while_other_heavy_work_freezes_north_1650() {
    let leases = [heavy("a", "land-check"), heavy("b", "gate:worker"), heavy("c", "land-offload")];
    let frozen = BTreeSet::from([leases[2].unit()]);
    let observed = Observed { gaming: false, pause_heavy: true, processes: &[], leases: &leases, frozen: &frozen };
    let planned = plan(&observed, |_| true, |_| true);
    assert_eq!(planned.freeze, BTreeSet::from([leases[1].unit()]), "on 2026-11-18 battery shedding froze the lander's after-landing checks, so three runs passed their deadline");
    assert_eq!(planned.thaw, BTreeSet::from([leases[2].unit()]));
    assert!(planned.stop.is_empty());
}
