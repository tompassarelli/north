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

fn quiet(proc: &Path, now: u64, seat_idle: Option<bool>) -> (State, mode::Stepped) {
    mode::step(&State::default(), &observe(proc, now, seat_idle), &Limits::bundled())
}

#[test]
fn an_open_game_with_no_lease_leaves_away_or_desktop_and_is_named_as_a_fact() {
    let root = tempfile::tempdir().unwrap();
    let quiet_table = root.path().join("quiet");
    desktop(&quiet_table);
    let played = root.path().join("played");
    desktop(&played);
    warcraft(&played);
    assert_eq!(quiet(&quiet_table, START, Some(true)).0.mode, Mode::Away);
    assert_eq!(quiet(&quiet_table, START, Some(false)).0.mode, Mode::Desktop);
    let (open, stepped) = quiet(&played, START, Some(false));
    assert_eq!((open.mode, open.hold, stepped), (Mode::Desktop, None, mode::Stepped::default()));
    assert_eq!(open.reason, "the seat is in use and no playtest lease is on; Tom's game is open");
    assert_eq!(quiet(&played, START, Some(true)).0.mode, Mode::Away);
}

#[test]
fn each_mode_applies_its_table_row_and_play_confines_heavy_work_only_while_the_split_is_enforced() {
    let limits = Limits::bundled();
    assert_eq!(limits.effects(Mode::Away), Effects::default());
    assert_eq!(limits.effects(Mode::Desktop), Effects::default());
    let play = limits.effects(Mode::Play);
    assert_eq!(play, Effects { reserve: true, hold: true, pause_heavy: false });
    assert_eq!(play.confined(true), play);
    assert_eq!(play.confined(false), Effects { pause_heavy: true, ..play });
    assert_eq!(limits.effects(Mode::Desktop).confined(false), Effects::default());
    let split = game_reserve::Split { game_cpus: "0-3,12-15".into(), agent_cpus: "4-11,16-23".into() };
    assert_eq!(mode::confinement(play, true, Some(&split)), Some(16));
    assert_eq!(mode::confinement(play, true, None), None, "an unenforced split pauses heavy work instead");
    assert_eq!(mode::confinement(play, false, Some(&split)), None);
    assert_eq!(mode::confinement(Effects { pause_heavy: true, ..play }, true, Some(&split)), None);
    assert_eq!(mode::cpu_split(24, 8), ("0-3,12-15".to_owned(), "4-11,16-23".to_owned()));
    assert_eq!(mode::cpu_split(24, 12), ("0-5,12-17".to_owned(), "6-11,18-23".to_owned()));
}

#[test]
fn a_default_lease_checks_in_at_eight_minutes_and_returns_itself_at_ten_with_the_game_still_open() {
    let limits = Limits::bundled();
    let root = tempfile::tempdir().unwrap();
    let played = root.path().join("played");
    desktop(&played);
    warcraft(&played);
    let leased = mode::hold(&State::default(), START, "test wisp", limits.lease_minutes, &limits);
    assert_eq!(leased.hold.as_ref().map(|hold| (hold.until, hold.minutes)), Some((START + 600, 10)));
    let (playing, stepped) = mode::step(&leased, &observe(&played, START + 479, Some(false)), &limits);
    assert_eq!((playing.mode, stepped.notice), (Mode::Play, None));
    let (warned, stepped) = mode::step(&playing, &observe(&played, START + 480, Some(false)), &limits);
    let until = warned.hold.as_ref().map(|hold| hold.until).unwrap();
    let notice = stepped.notice.unwrap();
    assert!(notice.starts_with("Playtest lease ends at ") && notice.ends_with(". Reply more to add 10 min."), "{notice}");
    let (still, stepped) = mode::step(&warned, &observe(&played, START + 540, Some(false)), &limits);
    assert_eq!((still.mode, stepped.notice), (Mode::Play, None), "the check-in is sent once");
    let (returned, stepped) = mode::step(&still, &observe(&played, until, Some(false)), &limits);
    assert_eq!((returned.mode, returned.hold.is_none()), (Mode::Desktop, true));
    let notice = stepped.notice.unwrap();
    assert!(notice.starts_with("Playtest lease returned at ") && notice.ends_with("; the laptop is working again."), "{notice}");
    let (after, _) = mode::step(&returned, &observe(&played, until + 30, Some(false)), &limits);
    assert_eq!(after.mode, Mode::Desktop, "the open game does not start a new lease");
}

#[test]
fn extending_adds_the_leases_own_length_and_resets_the_check_in_and_no_lease_exceeds_the_cap() {
    let limits = Limits::bundled();
    let leased = mode::hold(&State::default(), START, "test wisp online", 40, &limits);
    let mut warned = leased.clone();
    if let Some(hold) = warned.hold.as_mut() {
        hold.warned = true;
    }
    let extended = mode::extend(&warned, START + 2300, &limits).unwrap();
    let hold = extended.hold.unwrap();
    assert_eq!((hold.until, hold.minutes, hold.warned, hold.since), (START + 4800, 40, false, START));
    assert_eq!(mode::extend(&leased, START + 2400, &limits), None, "an ended lease cannot be extended");
    assert_eq!(mode::extend(&State::default(), START, &limits), None);
    let capped = mode::hold(&State::default(), START, "test wisp", 600, &limits);
    assert_eq!(capped.hold.map(|hold| hold.until), Some(START + limits.cap_seconds));
    let released = mode::release(&leased, START + 60);
    assert_eq!((released.mode, released.hold), (Mode::Desktop, None));
}

#[test]
fn the_play_reserve_widens_past_its_pressure_target_and_narrows_after_a_calm_period_within_its_clamp() {
    let limits = Limits::bundled();
    let held = mode::hold(&State::default(), START, "test wisp", 120, &limits);
    let seen = |now: u64, psi: f64| Observation { now, game_running: true, seat_idle: Some(false), protected_psi: Some(psi) };
    let (first, stepped) = mode::step(&held, &seen(START, 2.0), &limits);
    assert_eq!((first.play_cpus, stepped.adjustment), (Some(8), None));
    let (wide, stepped) = mode::step(&first, &seen(START + 30, 9.0), &limits);
    assert_eq!(stepped.adjustment.map(|change| (change.from, change.to)), Some((8, 10)));
    let mut state = wide;
    for tick in 2..10 {
        state = mode::step(&state, &seen(START + 30 * tick, 30.0), &limits).0;
    }
    assert_eq!(state.play_cpus, Some(12));
    let (calm, stepped) = mode::step(&state, &seen(START + 1000, 1.0), &limits);
    assert_eq!(stepped.adjustment, None);
    let (narrow, stepped) = mode::step(&calm, &seen(START + 1600, 1.0), &limits);
    assert_eq!(stepped.adjustment.map(|change| (change.from, change.to)), Some((12, 10)));
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
