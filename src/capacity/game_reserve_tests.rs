use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::game_reserve::{self, Settings};

const USER: &str = "/user.slice/user-1000.slice/user@1000.service";
const UPTIME: f64 = 1000.0;

fn add(proc: &Path, pid: i32, ppid: i32, cgroup: &str, args: &str, age: f64) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(&dir).unwrap();
    let ticks = ((UPTIME - age) * 100.0).round() as u64;
    let fields = format!("S {ppid} 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 {ticks} 0 0");
    fs::write(dir.join("stat"), format!("{pid} (proc {pid}) {fields}\n")).unwrap();
    fs::write(dir.join("cgroup"), format!("0::{cgroup}\n")).unwrap();
    fs::write(dir.join("cmdline"), format!("{args}\0")).unwrap();
}

fn settings(root: &Path) -> Settings {
    let proc = root.join("proc");
    fs::create_dir_all(&proc).unwrap();
    fs::write(proc.join("uptime"), format!("{UPTIME:.2} 5000.00\n")).unwrap();
    let log = root.join("calls.log");
    for tool in ["systemctl", "busctl"] {
        let script = root.join(tool);
        fs::write(&script, format!("#!/bin/sh\necho \"{tool} $*\" >> '{}'\n", log.display())).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        crate::foundation::shim::wait_until_executable(&script);
    }
    let _ = fs::remove_file(&log);
    Settings {
        proc,
        state: root.join("state.json"),
        systemctl: root.join("systemctl").display().to_string(),
        busctl: root.join("busctl").display().to_string(),
        affinity_log: Some(root.join("pins.log")),
        game_cpus: "0-3,12-15".into(),
        agent_cpus: "4-11,16-23".into(),
        agent_memory_max: "80G".into(),
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

