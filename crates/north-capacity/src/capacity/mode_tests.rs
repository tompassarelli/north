#![cfg(test)]

use std::fs;
use std::path::Path;

use super::agent_load::Host;
use super::cpus;
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

// KEEP(bug): on 2026-11-08 agent.slice held 22 of 24 CPUs on battery at load 87 because the quota followed the mode alone, and a heat tier then latched a charging laptop in use at 6 CPUs; north#834
#[test]
fn agent_slice_quota_is_22_cores_in_use_or_idle_on_mains_and_drops_only_on_battery() {
    let limits = Limits::bundled();
    let desk = limits.effects(Mode::Desktop);
    let away = limits.effects(Mode::Away);
    let quota = |effects, battery| mode_tick::agent_quota_percent(effects, battery, false, None, &limits);
    assert_eq!(quota(desk, Some(56)), 600, "in use on battery");
    assert_eq!(quota(desk, None), 2200, "in use on AC");
    assert_eq!(quota(away, None), 2200, "idle on AC");
    assert_eq!(quota(limits.effects(Mode::Play), Some(56)), 600, "a playtest on battery");
}

// KEEP(bug): on 2026-11-08 the laptop in use on battery held agent.slice at 22 of 24 CPUs at load 87 and drained the battery under an underpowered charger; the tick must set 600 on the battery reading and 2200 again on mains; north#834
#[test]
fn the_mode_tick_sets_600_on_a_battery_reading_while_in_use_and_2200_again_on_mains() {
    let root = tempfile::tempdir().unwrap();
    let quotas = |calls: Vec<String>| calls.into_iter().filter(|call| call.contains("CPUQuota=")).collect::<Vec<_>>();
    let on_battery = quotas(tick_with(root.path(), Some(false), Some(56), 1_000));
    assert_eq!(on_battery.last().map(String::as_str), Some("systemctl --user set-property --runtime agent.slice CPUQuota=600%"));
    let on_mains = quotas(tick_with(root.path(), Some(false), None, 1_030));
    assert_eq!(on_mains.last().map(String::as_str), Some(BUILD_HOST_QUOTA), "mains returns the 22 core quota while the seat stays in use");
}

// KEEP(bug): on 2026-11-08 a CPU sensor reading of 88 to 99 C held agent.slice at 600% for 21175 s on a charging laptop, because Tctl swings 77 to 100 C within seconds under rustc and the heat rule judged one reading; north#834
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
    assert_eq!(mode_tick::agent_quota_percent(away, None, false, relief, &limits), 400, "the tick keeps the relief");
    assert_eq!(relieved.relief.map(|relief| relief.until), Some(1_000 + 30 * 60));
}
