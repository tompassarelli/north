use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rustix::process::Signal;

use super::game_reserve::{self, Settings};
use super::mode::{self, Effects, Limits, Mode, Observation, State};
use super::teardown::{self, Names, Signaller};

const USER: &str = "/user.slice/user-1000.slice/user@1000.service";
const START: u64 = 1_800_000_000;

fn process(proc: &Path, pid: i32, ppid: i32, comm: &str, cgroup: &str, args: &str) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("stat"), format!("{pid} ({comm}) S {ppid} 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 100 0 0\n")).unwrap();
    fs::write(dir.join("cgroup"), format!("0::{USER}/{cgroup}\n")).unwrap();
    fs::write(dir.join("cmdline"), format!("{}\0", args.replace(' ', "\0"))).unwrap();
}

fn reserve(proc: &Path) -> Settings {
    fs::create_dir_all(proc).unwrap();
    fs::write(proc.join("uptime"), "100000.00 5000.00\n").unwrap();
    Settings {
        proc: proc.to_path_buf(),
        cgroup: PathBuf::new(),
        state: proc.join("state.json"),
        systemctl: String::new(),
        busctl: String::new(),
        affinity_log: None,
        game_cpus: String::new(),
        agent_cpus: String::new(),
        agent_memory_max: String::new(),
        idle_seconds: 600,
    }
}

fn desktop(proc: &Path) {
    process(proc, 1, 0, "systemd", "init.scope", "/sbin/init");
    process(proc, 20, 1, "firefox", "app.slice/app-firefox-1.scope", "/usr/bin/firefox");
    process(proc, 21, 1, "kitty", "app.slice/app-kitty-1.scope", "/usr/bin/kitty");
    process(proc, 22, 21, "fish", "app.slice/app-kitty-1.scope", "/usr/bin/fish");
    process(proc, 40, 1, "claude", "agent.slice/agent-session-1.scope", "/usr/bin/claude");
}

fn warcraft(proc: &Path) {
    let app = "app.slice/app-steam-1.scope";
    process(proc, 12, 1, "Battle.net.exe", app, "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe");
    process(proc, 13, 12, "Warcraft III.ex", app, "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch");
}

fn observe(proc: &Path, now: u64, seat_idle: Option<bool>) -> Observation {
    Observation { now, game_running: game_reserve::playing(&reserve(proc)), seat_idle, protected_psi: None }
}

#[test]
fn the_tick_classifies_away_desktop_and_play_from_process_tables_and_seat_idle() {
    let limits = Limits::bundled();
    let root = tempfile::tempdir().unwrap();
    let quiet = root.path().join("quiet");
    desktop(&quiet);
    let played = root.path().join("played");
    desktop(&played);
    warcraft(&played);
    let (away, _) = mode::step(&State::default(), &observe(&quiet, START, Some(true)), &limits);
    let (busy, _) = mode::step(&State::default(), &observe(&quiet, START, Some(false)), &limits);
    let (play, _) = mode::step(&State::default(), &observe(&played, START, Some(true)), &limits);
    assert_eq!((away.mode, busy.mode, play.mode), (Mode::Away, Mode::Desktop, Mode::Play));
    assert_eq!(play.reason, "Tom's game is running");
    let agent_only = root.path().join("agent-only");
    desktop(&agent_only);
    process(&agent_only, 50, 40, "Warcraft III.ex", "agent.slice/agent-session-1.scope", "C:\\Warcraft III\\Warcraft III.exe");
    assert_eq!(mode::step(&State::default(), &observe(&agent_only, START, Some(false)), &limits).0.mode, Mode::Desktop);
}

#[test]
fn each_mode_applies_its_table_row() {
    let limits = Limits::bundled();
    assert_eq!(limits.effects(Mode::Away), Effects::default());
    assert_eq!(limits.effects(Mode::Desktop), Effects::default());
    assert_eq!(limits.effects(Mode::Play), Effects { reserve: true, hold: true, pause_heavy: true });
    assert_eq!(mode::cpu_split(24, 8), ("0-3,12-15".to_owned(), "4-11,16-23".to_owned()));
    assert_eq!(mode::cpu_split(24, 12), ("0-5,12-17".to_owned(), "6-11,18-23".to_owned()));
}

#[test]
fn play_releases_ten_minutes_after_the_last_game_process_exits() {
    let limits = Limits::bundled();
    let root = tempfile::tempdir().unwrap();
    let played = root.path().join("played");
    desktop(&played);
    warcraft(&played);
    let quiet = root.path().join("quiet");
    desktop(&quiet);
    let (playing, _) = mode::step(&State::default(), &observe(&played, START, Some(false)), &limits);
    let exited = START + 30;
    let (lingering, _) = mode::step(&playing, &observe(&quiet, exited, Some(false)), &limits);
    assert_eq!(lingering.mode, Mode::Play);
    let (still, _) = mode::step(&lingering, &observe(&quiet, START + 599, Some(false)), &limits);
    assert_eq!(still.mode, Mode::Play);
    let (released, _) = mode::step(&still, &observe(&quiet, START + 600, Some(false)), &limits);
    assert_eq!(released.mode, Mode::Desktop);
    assert_eq!(released.since, START + 600);
}

#[test]
fn a_phrase_hold_expires_by_itself_after_two_hours_or_on_release() {
    let limits = Limits::bundled();
    let root = tempfile::tempdir().unwrap();
    let quiet = root.path().join("quiet");
    desktop(&quiet);
    let held = mode::hold(&State::default(), START, "test wisp", &limits);
    let (during, _) = mode::step(&held, &observe(&quiet, START + 7199, Some(true)), &limits);
    assert_eq!(during.mode, Mode::Play);
    assert!(during.reason.starts_with("quiet hold from Tom's prompt \"test wisp\""));
    let (expired, _) = mode::step(&during, &observe(&quiet, START + 7200, Some(true)), &limits);
    assert_eq!((expired.mode, expired.hold.is_none()), (Mode::Away, true));
    let released = mode::release(&during, START + 60);
    let (after, _) = mode::step(&released, &observe(&quiet, START + 61, Some(false)), &limits);
    assert_eq!(after.mode, Mode::Desktop);
}

#[test]
fn the_play_reserve_widens_past_its_pressure_target_and_narrows_after_a_calm_period_within_its_clamp() {
    let limits = Limits::bundled();
    let seen = |now: u64, psi: f64| Observation { now, game_running: true, seat_idle: Some(false), protected_psi: Some(psi) };
    let (first, adjustment) = mode::step(&State::default(), &seen(START, 2.0), &limits);
    assert_eq!((first.play_cpus, adjustment), (Some(8), None));
    let (wide, adjustment) = mode::step(&first, &seen(START + 30, 9.0), &limits);
    assert_eq!(adjustment.map(|change| (change.from, change.to)), Some((8, 10)));
    let mut state = wide;
    for tick in 2..10 {
        state = mode::step(&state, &seen(START + 30 * tick, 30.0), &limits).0;
    }
    assert_eq!(state.play_cpus, Some(12));
    let (calm, adjustment) = mode::step(&state, &seen(START + 1000, 1.0), &limits);
    assert_eq!(adjustment, None);
    let (narrow, adjustment) = mode::step(&calm, &seen(START + 1600, 1.0), &limits);
    assert_eq!(adjustment.map(|change| (change.from, change.to)), Some((12, 10)));
    assert_eq!(narrow.play_cpus, Some(10));
}

#[derive(Default)]
struct Recorder {
    sent: Vec<(i32, Signal)>,
    stubborn: BTreeSet<i32>,
}

impl Signaller for Recorder {
    fn send(&mut self, pid: i32, signal: Signal) {
        self.sent.push((pid, signal));
    }

    fn alive(&self, pid: i32) -> bool {
        self.stubborn.contains(&pid) && !self.sent.contains(&(pid, Signal::Kill))
    }

    fn wait(&mut self, _: Duration) {}
}

fn names() -> Names {
    Names::bundled()
}

#[test]
fn done_testing_closes_warcraft_iii_and_battle_net_and_nothing_else() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    desktop(&proc);
    warcraft(&proc);
    process(&proc, 23, 22, "rg", "app.slice/app-kitty-1.scope", "/usr/bin/rg Warcraft III.exe");
    let targets = teardown::targets(&teardown::table(&proc), &names());
    assert_eq!(targets.all(), BTreeSet::from([12, 13]));
    let mut recorder = Recorder { stubborn: BTreeSet::from([13]), ..Recorder::default() };
    let line = teardown::run(&proc, &names(), &mut recorder, Duration::from_secs(10));
    assert_eq!(recorder.sent, vec![(12, Signal::Term), (13, Signal::Term), (13, Signal::Kill)]);
    assert_eq!(line, "closed Warcraft III (1 processes), its launcher (1 processes), 1 force-killed after the grace period");
}

#[test]
fn a_native_test_teardown_closes_only_the_native_client_scope() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    desktop(&proc);
    let scope = "native.slice/native-run-7.scope";
    process(&proc, 60, 1, "north", scope, "/usr/bin/north capacity run --class native");
    process(&proc, 61, 60, "wisp-window", scope, "/home/tom/code/wisp/target/release/wisp-window --map test");
    process(&proc, 70, 40, "cargo", "agent.slice/agent-session-1.scope", "/usr/bin/cargo test");
    let targets = teardown::targets(&teardown::table(&proc), &names());
    assert_eq!((targets.game.len(), targets.launcher.len()), (0, 0));
    assert_eq!(targets.native, BTreeSet::from([60, 61]));
    let mut recorder = Recorder::default();
    let line = teardown::run(&proc, &names(), &mut recorder, Duration::from_secs(10));
    assert_eq!(recorder.sent, vec![(60, Signal::Term), (61, Signal::Term)]);
    assert_eq!(line, "closed the native test client (2 processes)");
}
