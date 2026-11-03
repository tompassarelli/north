use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::game_reserve::{self, Settings};

const USER: &str = "/user.slice/user-1000.slice/user@1000.service";
const UPTIME: f64 = 1000.0;
const SLICES: [&str; 6] = ["agent.slice", "background.slice", "control.slice", "native.slice", "user.slice", "builds.slice"];
const TOM_SLICES: [&str; 3] = ["app.slice", "session.slice", "game.slice"];
const NESTED: &str = "agent.slice/agent-capacity.slice";

fn add(proc: &Path, pid: i32, ppid: i32, cgroup: &str, args: &str, age: f64) {
    named(proc, pid, ppid, &format!("proc {pid}"), cgroup, args, age);
}

fn named(proc: &Path, pid: i32, ppid: i32, comm: &str, cgroup: &str, args: &str, age: f64) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(&dir).unwrap();
    let ticks = ((UPTIME - age) * 100.0).round() as u64;
    let fields = format!("S {ppid} 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 {ticks} 0 0");
    fs::write(dir.join("stat"), format!("{pid} ({comm}) {fields}\n")).unwrap();
    fs::write(dir.join("cgroup"), format!("0::{cgroup}\n")).unwrap();
    fs::write(dir.join("cmdline"), format!("{args}\0")).unwrap();
}

fn settings(root: &Path) -> Settings {
    let proc = root.join("proc");
    fs::create_dir_all(&proc).unwrap();
    fs::write(proc.join("uptime"), format!("{UPTIME:.2} 5000.00\n")).unwrap();
    let log = root.join("calls.log");
    let cgroup = root.join("cgroup");
    for slice in SLICES.iter().chain(&TOM_SLICES) {
        fs::create_dir_all(cgroup.join(slice)).unwrap();
        fs::write(cgroup.join(slice).join("cpuset.cpus.effective"), "0-23\n").unwrap();
    }
    fs::create_dir_all(cgroup.join("init.scope")).unwrap();
    fs::create_dir_all(cgroup.join(NESTED)).unwrap();
    fs::write(cgroup.join(NESTED).join("cpuset.cpus.effective"), "0-23\n").unwrap();
    let kernel = format!(
        "[ \"$2\" = set-property ] || exit 0\nslice=$4; shift 4\nfor p; do case $p in AllowedCPUs=*) printf '%s\\n' \"${{p#AllowedCPUs=}}\" > \"$(find '{}' -type d -name \"$slice\")\"/cpuset.cpus.effective 2>/dev/null;; esac; done\n",
        cgroup.display()
    );
    for tool in ["systemctl", "busctl"] {
        let script = root.join(tool);
        let body = if tool == "systemctl" { kernel.as_str() } else { "" };
        fs::write(&script, format!("#!/bin/sh\necho \"{tool} $*\" >> '{}'\n{body}", log.display())).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        crate::foundation::shim::wait_until_executable(&script);
    }
    let _ = fs::remove_file(&log);
    Settings {
        proc,
        cgroup,
        state: root.join("state.json"),
        systemctl: root.join("systemctl").display().to_string(),
        busctl: root.join("busctl").display().to_string(),
        affinity_log: Some(root.join("pins.log")),
        game_cpus: "0-3,12-15".into(),
        agent_cpus: "4-11,16-23".into(),
        agent_memory_max: "80G".into(),
        idle_seconds: 600,
    }
}

fn launch(root: &Path, age: f64) -> (String, String) {
    let settings = settings(root);
    let app = format!("{USER}/app.slice/app-niri-rofi-1.scope");
    add(&settings.proc, 1, 0, "/init.scope", "systemd", UPTIME);
    add(&settings.proc, 12, 1, &app, "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe", UPTIME);
    add(&settings.proc, 13, 12, &app, "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch", age);
    add(&settings.proc, 14, 13, &app, "wineserver", age);
    add(&settings.proc, 50, 1, &format!("{USER}/agent.slice/agent-capacity.slice/run-1.scope"), "bun test", UPTIME);
    game_reserve::on(&settings, true).unwrap();
    let calls = fs::read_to_string(root.join("calls.log")).unwrap_or_default();
    let pins = fs::read_to_string(root.join("pins.log")).unwrap_or_default();
    (calls, pins)
}

#[test]
fn a_warcraft_iii_younger_than_a_minute_is_neither_moved_nor_pinned() {
    for age in [0.0, 1.0, 3.0, 59.9] {
        let root = tempfile::tempdir().unwrap();
        let (calls, pins) = launch(root.path(), age);
        assert!(!calls.contains("StartTransientUnit"), "age {age}: {calls}");
        assert_eq!(pins, "pin 50 4-23:16\n", "age {age}");
        assert!(calls.contains("set-property --runtime agent.slice AllowedCPUs=4-11,16-23 MemoryMax=80G"), "age {age}: {calls}");
    }
}

#[test]
fn a_warcraft_iii_a_minute_or_older_is_moved_and_pinned_to_the_game_cores() {
    for age in [60.0, 61.0, 900.0] {
        let root = tempfile::tempdir().unwrap();
        let (calls, pins) = launch(root.path(), age);
        let moved = calls.lines().find(|line| line.contains("StartTransientUnit")).unwrap_or_default();
        assert!(moved.ends_with("fail 2 PIDs au 2 13 14 Slice s game-reserve.slice 0"), "age {age}: {calls}");
        assert!(moved.contains(" game-reserve-") && moved.contains("-13.scope "), "age {age}: {moved}");
        assert_eq!(pins, "pin 13 0-15:8\npin 14 0-15:8\npin 50 4-23:16\n", "age {age}");
    }
}


fn effective(settings: &Settings, slice: &str) -> String {
    fs::read_to_string(settings.cgroup.join(slice).join("cpuset.cpus.effective")).unwrap().trim().to_owned()
}

fn spread(spec: &str) -> Vec<usize> {
    spec.split(',')
        .flat_map(|part| {
            let (low, high) = part.split_once('-').unwrap_or((part, part));
            low.parse::<usize>().unwrap()..=high.parse::<usize>().unwrap()
        })
        .collect()
}

fn reserved(root: &Path) -> Settings {
    let settings = settings(root);
    add(&settings.proc, 50, 1, &format!("{USER}/agent.slice/agent-capacity.slice/run-1.scope"), "bun test", UPTIME);
    add(&settings.proc, 51, 1, &format!("{USER}/user.slice/libpod-1.scope/container"), "Runner.Listener", UPTIME);
    add(&settings.proc, 52, 1, &format!("{USER}/background.slice/gitleaks-1.scope"), "gitleaks dir", UPTIME);
    add(&settings.proc, 53, 1, &format!("{USER}/app.slice/app-niri-ghostty-1.scope"), "bash", UPTIME);
    game_reserve::on(&settings, true).unwrap();
    settings
}

#[test]
fn with_the_reserve_on_no_slice_but_toms_runs_on_a_game_cpu() {
    let root = tempfile::tempdir().unwrap();
    let settings = reserved(root.path());
    let game = spread(&settings.game_cpus);
    for slice in fs::read_dir(&settings.cgroup).unwrap().flatten().filter_map(|entry| entry.file_name().into_string().ok()) {
        if !slice.ends_with(".slice") {
            continue;
        }
        let cpus = spread(&effective(&settings, &slice));
        if TOM_SLICES.contains(&slice.as_str()) {
            assert_eq!(cpus, spread("0-23"), "{slice} is Tom's and keeps every CPU");
        } else {
            assert!(cpus.iter().all(|cpu| !game.contains(cpu)), "{slice} may run on a game CPU: {cpus:?}");
        }
    }
    assert_eq!(effective(&settings, NESTED), "4-11,16-23", "a slice nested in an agent slice is confined too");
    let pins = fs::read_to_string(root.path().join("pins.log")).unwrap();
    assert_eq!(pins, "pin 50 4-23:16\npin 51 4-23:16\npin 52 4-23:16\n");
    let (enforced, line) = game_reserve::status(&settings);
    assert!(enforced, "{line}");
}

#[test]
fn off_gives_every_confined_slice_every_cpu_again() {
    let root = tempfile::tempdir().unwrap();
    let settings = reserved(root.path());
    game_reserve::off(&settings).unwrap();
    for slice in SLICES.iter().chain(&TOM_SLICES).chain(&[NESTED]) {
        assert_eq!(effective(&settings, slice), "0-23", "{slice}");
    }
    let calls = fs::read_to_string(root.path().join("calls.log")).unwrap();
    assert!(calls.contains("set-property --runtime agent.slice AllowedCPUs=0-23 MemoryMax=infinity"), "{calls}");
    assert!(!calls.contains("AllowedCPUs= ") && !calls.lines().any(|line| line.ends_with("AllowedCPUs=")), "{calls}");
}

#[test]
fn status_reports_a_reserve_whose_slices_still_reach_the_game_cpus_as_not_enforced() {
    let root = tempfile::tempdir().unwrap();
    let settings = settings(root.path());
    fs::write(&settings.state, r#"{"on":true,"auto":true}"#).unwrap();
    let (enforced, line) = game_reserve::status(&settings);
    assert!(!enforced, "{line}");
    assert!(
        line.ends_with("NOT ENFORCED: game CPUs open to agent-capacity.slice, agent.slice, background.slice, builds.slice, control.slice, native.slice, user.slice"),
        "{line}"
    );
}

#[test]
fn toms_game_is_warcraft_iii_or_a_wisp_native_window_outside_the_agent_slices() {
    let app = format!("{USER}/app.slice/makgora-tom-1.service");
    let agent = format!("{USER}/agent.slice/agent-capacity.slice/run-1.scope");
    let cases = [
        ("bun", app.as_str(), "bun scripts/wisp.ts play --standalone --window native --map current.w3x", true),
        ("bun", app.as_str(), "bun wisp play --standalone --window native", true),
        ("bun", agent.as_str(), "bun scripts/wisp.ts play --standalone --window native", false),
        ("bun", app.as_str(), "bun scripts/wisp.ts play --standalone --window headless", false),
        ("bun", app.as_str(), "bun scripts/wisp.ts net --window native", false),
        ("bun", app.as_str(), "bun scripts/wisp.ts play --standalone --window native --headless --frames 150", false),
        ("bun", app.as_str(), "bun machine-capacity.mjs run --class gpu -- bun scripts/wisp.ts play --standalone --window native", false),
        ("Warcraft III.ex", app.as_str(), "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch", true),
        ("Warcraft III.ex", &format!("{USER}/native.slice/agent-capacity-1.scope"), "Warcraft III.exe -launch", false),
        ("bash", app.as_str(), "bash -c sleep 30; : Warcraft III.exe", false),
    ];
    for (comm, cgroup, args, want) in cases {
        let root = tempfile::tempdir().unwrap();
        let settings = settings(root.path());
        named(&settings.proc, 40, 1, comm, cgroup, args, UPTIME);
        assert_eq!(game_reserve::playing(&settings), want, "{comm} in {cgroup}: {args}");
    }
}

#[test]
fn auto_keeps_the_reserve_on_while_toms_wisp_window_runs_and_releases_it_after_the_idle_period() {
    let root = tempfile::tempdir().unwrap();
    let mut settings = settings(root.path());
    let app = format!("{USER}/app.slice/makgora-tom-1.service");
    named(&settings.proc, 40, 1, "bun", &app, "bun scripts/wisp.ts play --standalone --window native", UPTIME);
    game_reserve::auto(&settings).unwrap();
    let state = || fs::read_to_string(root.path().join("state.json")).unwrap();
    assert!(state().contains(r#""on":true"#) && state().contains(r#""auto":true"#), "{}", state());
    assert!(game_reserve::status(&settings).1.ends_with("Tom's Wisp native window running"), "{}", game_reserve::status(&settings).1);
    settings.idle_seconds = 0;
    game_reserve::auto(&settings).unwrap();
    assert!(state().contains(r#""on":true"#), "a running window keeps the reserve on: {}", state());
    fs::remove_dir_all(settings.proc.join("40")).unwrap();
    settings.idle_seconds = 600;
    game_reserve::auto(&settings).unwrap();
    assert!(state().contains(r#""on":true"#), "the reserve outlasts a window gone for less than the idle period: {}", state());
    settings.idle_seconds = 0;
    game_reserve::auto(&settings).unwrap();
    assert_eq!(state(), r#"{"on":false}"#);
    game_reserve::on(&settings, false).unwrap();
    game_reserve::auto(&settings).unwrap();
    assert!(state().contains(r#""auto":false"#), "a reserve switched on by hand is never switched off by auto: {}", state());
}
