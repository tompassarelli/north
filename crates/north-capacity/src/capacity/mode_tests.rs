#![cfg(test)]

use std::fs;
use std::path::Path;

use super::agent_load::Host;
use super::cpus;
use super::game_reserve_tests::settings;
use super::mode::{self, Limits, Mode};

const BUILD_HOST_QUOTA: &str = "systemctl --user set-property --runtime agent.slice CPUQuota=2200%";
const PLAY_QUOTA: &str = "systemctl --user set-property --runtime agent.slice CPUQuota=1200%";

fn host(root: &Path) -> Host {
    Host { proc: root.join("proc"), cgroup: root.join("cgroup"), capacity: root.join("capacity"), game_reserve: root.join("game-reserve.json"), systemctl: root.join("systemctl").display().to_string(), uid: 1000 }
}

fn tick(root: &Path, idle: Option<bool>, now: u64) -> Vec<String> {
    mode::tick_in(&host(root), idle, mode::Power::default(), settings(root), &root.join("mode"), now);
    fs::read_to_string(root.join("calls.log")).unwrap_or_default().lines().map(str::to_owned).collect()
}

// KEEP(bug): at the desk with no playtest lease the laptop held agents to 18 threads and sent its sessions' builds to the GPU host first, sitting at load 1.4 of 24 beside the GPU host at 52 on 2026-11-07; only a playtest lease drops it from the build host quota; north#740
#[test]
fn agent_slice_quota_stays_at_the_build_host_quota_at_the_desk_and_drops_only_under_a_playtest_lease() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("mode");
    let limits = Limits::bundled();
    let quotas = |calls: &[String]| calls.iter().filter(|call| call.contains("CPUQuota=")).cloned().collect::<Vec<_>>();
    assert_eq!(quotas(&tick(root.path(), Some(true), 1_000)), [BUILD_HOST_QUOTA]);
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_030)), Vec::<String>::new(), "sitting down at the desk keeps the build host quota");
    let desk = mode::read(&dir);
    assert_eq!(desk.mode, Mode::Desktop);
    assert_eq!(cpus::aggregate_cpus("attended", 24.0, Some(f64::from(limits.build_host_quota_percent) / 100.0)), 22.0, "admission at the desk leases up to the 22 CPUs agent.slice holds");
    assert!(mode::builds_here_at(&desk, &limits, 1_040));
    assert!(!mode::builds_here_at(&desk, &limits, 1_030 + limits.build_host_fresh_seconds + 1));
    assert!(mode::leases_own_builds_at(false, &desk, &limits, 1_040));
    assert!(!mode::leases_own_builds_at(true, &desk, &limits, 1_040), "a land check or replay never reads the live attendance");
    mode::write(&dir, &mode::hold(&desk, 1_050, "playtest", 30, &limits)).unwrap();
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_060)), [PLAY_QUOTA]);
    assert!(!mode::builds_here_at(&mode::read(&dir), &limits, 1_070));
    mode::write(&dir, &mode::release(&mode::read(&dir), 1_080)).unwrap();
    assert_eq!(quotas(&tick(root.path(), Some(false), 1_090)), [BUILD_HOST_QUOTA]);
}

// KEEP(bug): with the seat in use the laptop counted as a land-check peer and ran the lander's candidate checks beside gex45, at load 87 on 2026-11-08; only an idle seat makes it one; north#833
#[test]
fn only_an_away_laptop_takes_land_checks_beside_the_build_hosts() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("mode");
    let limits = Limits::bundled();
    tick(root.path(), Some(true), 1_000);
    assert!(mode::takes_land_checks_at(&mode::read(&dir), &limits, 1_010));
    tick(root.path(), Some(false), 1_020);
    let desk = mode::read(&dir);
    assert!(mode::builds_here_at(&desk, &limits, 1_030), "the desk still builds its own sessions' compiles");
    assert!(!mode::takes_land_checks_at(&desk, &limits, 1_030));
}

// KEEP(bug): on 2026-11-08 agent.slice held 22 of 24 CPUs on battery at 81 to 90 C while the laptop ran at load 87, because the quota followed the mode alone; north#834
#[test]
fn agent_slice_quota_drops_to_the_battery_or_hot_quota_whatever_the_mode() {
    let limits = Limits::bundled();
    let desk = limits.effects(Mode::Desktop);
    let away = limits.effects(Mode::Away);
    let quota = |effects, battery, cpu_celsius, previous| mode::agent_quota_percent(effects, mode::Power { battery, cpu_celsius }, None, previous, &limits);
    assert_eq!(quota(desk, Some(56), Some(70.0), None), 600, "in use on battery");
    assert_eq!(quota(desk, None, Some(70.0), None), 2200, "in use on AC");
    assert_eq!(quota(away, None, Some(70.0), None), 2200, "idle on AC");
    assert_eq!(quota(away, None, Some(91.0), Some(2200)), 600, "hot");
    assert_eq!(quota(away, None, Some(85.0), Some(600)), 600, "a hot quota holds until the CPU cools");
    assert_eq!(quota(away, None, Some(79.0), Some(600)), 2200, "a cooled CPU lifts the hot quota");
    assert_eq!(quota(limits.effects(Mode::Play), Some(56), None, None), 600, "a playtest on battery");
}

// KEEP(bug): on 2026-11-08 the proxy could not lower agent.slice under load because a raw systemctl set-property waited on the capacity admission it was meant to relieve; north#834
#[test]
fn a_relief_lowers_the_agent_quota_at_once_never_raises_it_and_lapses() {
    let limits = Limits::bundled();
    let away = limits.effects(Mode::Away);
    let applied = mode::State { agent_quota_percent: Some(2200), ..mode::State::default() };
    let (relieved, lower) = mode::relieve(&applied, 400, 30, 1_000);
    assert_eq!(lower, Some(400));
    assert_eq!(mode::relieve(&applied, 2400, 30, 1_000).1, None, "a relief above the applied quota raises nothing");
    let relief = relieved.relief.map(|relief| relief.percent);
    assert_eq!(mode::agent_quota_percent(away, mode::Power::default(), relief, Some(400), &limits), 400, "the tick keeps the relief");
    assert_eq!(relieved.relief.map(|relief| relief.until), Some(1_000 + 30 * 60));
}
