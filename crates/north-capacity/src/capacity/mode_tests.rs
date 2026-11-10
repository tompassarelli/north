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
}

#[test]
fn agent_slice_quota_is_12_cores_in_use_and_22_idle_on_mains_and_drops_only_on_battery() {
    let limits = Limits::bundled();
    let desk = limits.effects(Mode::Desktop);
    let away = limits.effects(Mode::Away);
    let quota = |effects, battery| mode_tick::agent_quota_percent(effects, battery, false, None, &limits);
    assert_eq!(quota(desk, Some(56)), 600, "in use on battery");
    assert_eq!(quota(desk, None), 1200, "in use on AC");
    assert_eq!(quota(away, None), 2200, "idle on AC");
    assert_eq!(quota(limits.effects(Mode::Play), Some(56)), 600, "a playtest on battery");
}

#[test]
fn the_mode_tick_sets_600_on_a_battery_reading_while_in_use_and_2200_again_on_mains() {
    let root = tempfile::tempdir().unwrap();
    let quotas = |calls: Vec<String>| calls.into_iter().filter(|call| call.contains("CPUQuota=")).collect::<Vec<_>>();
    let on_battery = quotas(tick_with(root.path(), Some(false), Some(56), 1_000));
    assert_eq!(on_battery.last().map(String::as_str), Some("systemctl --user set-property --runtime agent.slice CPUQuota=600%"));
    let on_mains = quotas(tick_with(root.path(), Some(false), None, 1_030));
    assert_eq!(on_mains.last().map(String::as_str), Some(PLAY_QUOTA), "mains returns the 12 core quota while the seat stays in use");
}

#[test]
fn only_a_sixty_second_average_of_98_c_caps_agent_slice_to_half_the_cores_until_it_falls_below_90_c() {
    let limits = Limits::bundled();
    let away = limits.effects(Mode::Away);
    let mut state = mode::State::default();
    let mut run = |now: u64, celsius: f64| {
        let (heat, hot) = mode_tick::heat_step(&state, Some(celsius), now, &limits);
        state.heat = heat;
        state.hot = hot;
        mode_tick::agent_quota_percent(away, None, hot, None, &limits)
    };
    assert_eq!(run(0, 100.0), 1200, "a first reading at 100 C averages 100 C");
    assert_eq!(run(30, 77.0), 2200, "a swing to 77 C brings the 60 s average to 88.5 C, under the 90 C lift");
    assert_eq!(run(60, 99.0), 2200, "77 and 99 average 88 C");
    assert_eq!(run(90, 99.0), 1200, "two readings of 99 C average 99 C");
    assert_eq!(run(120, 92.0), 1200, "a hot cap holds at an average of 95.5 C");
    assert_eq!(run(150, 80.0), 2200, "an average of 86 C lifts it");
    assert!(!mode_tick::heat_step(&state, None, 180, &limits).1, "no sensor never caps");
}

#[test]
fn a_relief_lowers_the_agent_quota_at_once_never_raises_it_and_lapses() {
    let limits = Limits::bundled();
    let away = limits.effects(Mode::Away);
    let applied = mode::State { agent_quota_percent: Some(2200), ..mode::State::default() };
    let (relieved, lower) = mode::relieve(&applied, 400, 30, 1_000);
    assert_eq!(lower, Some(400));
    assert_eq!(mode::relieve(&applied, 2400, 30, 1_000).1, None, "a relief above the applied quota raises nothing");
    let relief = relieved.relief.map(|relief| relief.percent);
    assert_eq!(mode_tick::agent_quota_percent(away, None, false, relief, &limits), 400, "the tick keeps the relief");
    assert_eq!(relieved.relief.map(|relief| relief.until), Some(1_000 + 30 * 60));
}
