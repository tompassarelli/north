#![cfg(test)]

use std::fs;
use std::path::Path;

use super::agent_load::Host;
use super::game_reserve_tests::settings;
use super::mode::{self, Limits, Mode, State};

const AWAY_QUOTA: &str = "systemctl --user set-property --runtime agent.slice CPUQuota=2400%";
const ATTENDED_QUOTA: &str = "systemctl --user set-property --runtime agent.slice CPUQuota=1800%";

fn host(root: &Path) -> Host {
    Host { proc: root.join("proc"), cgroup: root.join("cgroup"), capacity: root.join("capacity"), game_reserve: root.join("game-reserve.json"), systemctl: root.join("systemctl").display().to_string(), uid: 1000 }
}

fn tick(root: &Path, idle: Option<bool>, now: u64) -> Vec<String> {
    mode::tick_in(&host(root), idle, settings(root), &root.join("mode"), now);
    fs::read_to_string(root.join("calls.log")).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn agent_slice_quota_rises_when_the_seat_goes_idle_and_returns_within_one_tick_of_input() {
    let root = tempfile::tempdir().unwrap();
    let quotas = |calls: &[String]| calls.iter().filter(|call| call.contains("CPUQuota=")).cloned().collect::<Vec<_>>();
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_000)), [ATTENDED_QUOTA]);
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_030)), [ATTENDED_QUOTA]);
    assert_eq!(quotas(&tick(root.path(), Some(true), 1_060)), [ATTENDED_QUOTA, AWAY_QUOTA]);
    assert_eq!(quotas(&tick(root.path(), Some(true), 1_090)), [ATTENDED_QUOTA, AWAY_QUOTA]);
    let limits = Limits::bundled();
    let away = mode::read(&root.path().join("mode"));
    assert!(mode::builds_here_at(&away, &limits, 1_100));
    assert!(!mode::builds_here_at(&away, &limits, 1_090 + limits.away_fresh_seconds + 1));
    assert!(!mode::builds_here_at(&State { mode: Mode::Play, ..away }, &limits, 1_100));
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_120)), [ATTENDED_QUOTA, AWAY_QUOTA, ATTENDED_QUOTA]);
}

