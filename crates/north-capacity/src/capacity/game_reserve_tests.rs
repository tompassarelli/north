#![cfg(test)]

use std::fs;
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
        crate::foundation::shim::write_script(&script, &format!("echo \"{tool} $*\" >> '{}'\n{body}", log.display()));
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

// KEEP(bug): Warcraft III is not moved or pinned until it has run a minute; regression of the black-screen launches (commit 318a62d33)
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

// KEEP(bug): Tom's game is Warcraft III or a Wisp native window outside the agent slices; regression of 8b58dbb52
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
