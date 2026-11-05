use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const USER: &str = "/user.slice/user-1000.slice/user@1000.service";
const WARCRAFT: &str = "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch";
const BATTLE_NET: &str = "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe";

struct Reserve {
    root: tempfile::TempDir,
}

impl Reserve {
    fn new() -> Self {
        let reserve = Self { root: tempfile::tempdir().unwrap() };
        for slice in ["agent", "background", "user", "app"] {
            fs::create_dir_all(reserve.path(&format!("cgroup/{slice}.slice"))).unwrap();
        }
        fs::create_dir_all(reserve.path("proc")).unwrap();
        fs::write(reserve.path("proc/uptime"), "1000.00 9000.00\n").unwrap();
        for tool in ["systemctl", "busctl"] {
            let script = reserve.path(tool);
            fs::write(&script, format!("#!/usr/bin/env bash\nprintf '{tool} %s\\n' \"$*\" >> {}\n", reserve.path("calls.log").display())).unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        reserve
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn add(&self, pid: u32, comm: &str, ppid: u32, group: &str, cmd: &str, start: u64) {
        let dir = self.path(&format!("proc/{pid}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("stat"), format!("{pid} ({comm}) S {ppid} 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 {start} 0\n")).unwrap();
        self.regroup(pid, group);
        fs::write(dir.join("cmdline"), format!("{cmd}\0")).unwrap();
    }

    fn regroup(&self, pid: u32, group: &str) {
        fs::write(self.path(&format!("proc/{pid}/cgroup")), format!("0::{group}\n")).unwrap();
    }

    fn run(&self, verb: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["capacity", "game-reserve", verb])
            .env("GAME_RESERVE_CGROUP", self.path("cgroup"))
            .env("GAME_RESERVE_AFFINITY_LOG", self.path("pins.log"))
            .env("GAME_RESERVE_PROC", self.path("proc"))
            .env("GAME_RESERVE_STATE", self.path("state.json"))
            .env("GAME_RESERVE_SYSTEMCTL", self.path("systemctl"))
            .env("GAME_RESERVE_BUSCTL", self.path("busctl"))
            .output()
            .unwrap()
    }

    fn confine(&self) {
        for slice in ["agent", "background", "user", "app"] {
            fs::write(self.path(&format!("cgroup/{slice}.slice/cpuset.cpus.effective")), "4-11,16-23\n").unwrap();
        }
    }

    fn calls(&self) -> Vec<String> {
        read_lines(&self.path("calls.log"))
    }

    fn pins(&self) -> String {
        let mut pins = read_lines(&self.path("pins.log"));
        pins.sort_by_key(|line| line.split_whitespace().nth(1).and_then(|pid| pid.parse::<u32>().ok()));
        pins.join(" ")
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path).unwrap_or_default().lines().map(str::to_owned).collect()
}

fn matching(lines: &[String], expected: &str) -> usize {
    lines.iter().filter(|line| line.as_str() == expected).count()
}

fn scope_moves(lines: &[String], pid: u32) -> usize {
    let tail = format!(" PIDs au 1 {pid} Slice s game-reserve.slice 0");
    lines.iter().filter(|line| line.starts_with("busctl ") && line.contains("StartTransientUnit ssa(sv)a(sa(sv)) game-reserve-") && line.ends_with(&tail)).count()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim_end().to_owned()
}

#[test]
fn the_game_reserve_confines_agents_and_moves_only_warcraft_into_its_slice() {
    let reserve = Reserve::new();
    let rofi = format!("{USER}/app.slice/app-niri-rofi-1.scope");
    reserve.add(1, "systemd", 0, "/init.scope", "systemd", 0);
    reserve.add(10, "steam", 1, &rofi, "steam", 0);
    reserve.add(11, "reaper", 10, &rofi, "reaper SteamLaunch", 0);
    reserve.add(12, "wine64-preloader", 11, &rofi, BATTLE_NET, 0);
    reserve.add(13, "Warcraft III.ex", 12, &rofi, WARCRAFT, 88000);
    reserve.add(20, "Warcraft III.ex", 1, &format!("{USER}/native.slice/agent-capacity-1.scope"), WARCRAFT, 0);
    reserve.add(30, "bash", 1, &format!("{USER}/app.slice/app-ghostty-1.scope"), "bash", 0);
    reserve.add(50, "bun", 1, &format!("{USER}/agent.slice/agent-capacity.slice/run-1.scope"), "bun test", 0);

    assert_eq!(reserve.run("status").status.code(), Some(1), "status exits non-zero before the reserve is on");

    let on = reserve.run("on");
    assert!(on.status.success(), "{}", String::from_utf8_lossy(&on.stderr));
    let calls = reserve.calls();
    assert_eq!(matching(&calls, "systemctl --user set-property --runtime agent.slice AllowedCPUs=4-11,16-23 MemoryMax=80G"), 1, "on confines agents to CCX1 with the memory cap: {calls:?}");
    assert_eq!(scope_moves(&calls, 13), 1, "on moves Warcraft III into game-reserve.slice: {calls:?}");
    for pid in [10, 11, 12, 20, 30, 50] {
        assert_eq!(scope_moves(&calls, pid), 0, "pid {pid} (Steam, Battle.net, a pool client or Tom's terminal) stays out of game-reserve.slice");
    }
    assert_eq!(reserve.pins(), "pin 13 0-15:8 pin 20 4-23:16 pin 50 4-23:16", "on pins Warcraft III to CCX0 and agent and pool client threads to CCX1");
    assert_eq!(
        matching(&calls, "systemctl --user set-property --runtime game-reserve.slice AllowedCPUs=0-3,12-15 CPUWeight=1000 IOWeight=1000 MemoryMin=4G MemoryLow=6G"),
        1,
        "on reserves CCX0, weights and the memory floor for the game"
    );

    reserve.regroup(13, &format!("{USER}/game-reserve.slice/game-reserve-1-13.scope"));
    reserve.add(40, "Warcraft III.ex", 12, &rofi, WARCRAFT, 90000);
    assert!(reserve.run("on").status.success());
    let calls = reserve.calls();
    assert_eq!(scope_moves(&calls, 40), 1, "a second on moves a Warcraft III started since: {calls:?}");
    assert_eq!(scope_moves(&calls, 13), 1, "a second on leaves the Warcraft III already inside the slice");
    reserve.regroup(40, &format!("{USER}/game-reserve.slice/game-reserve-2-40.scope"));

    let unconfined = reserve.run("status");
    assert_eq!(unconfined.status.code(), Some(1), "status exits non-zero while a slice still lets agents onto the game CPUs");
    assert_eq!(stdout(&unconfined), "game-reserve on: game CPUs 0-3,12-15, agents CPUs 4-11,16-23; Warcraft III 2/2 in game-reserve.slice; NOT ENFORCED: game CPUs open to agent.slice");

    reserve.confine();
    let status = reserve.run("status");
    assert_eq!(status.status.code(), Some(0), "{}", String::from_utf8_lossy(&status.stderr));
    assert_eq!(stdout(&status), "game-reserve on: game CPUs 0-3,12-15, agents CPUs 4-11,16-23; Warcraft III 2/2 in game-reserve.slice; ENFORCED: every agent slice's AllowedCPUs excludes the game CPUs 0-3,12-15");

    assert!(reserve.run("off").status.success());
    assert_eq!(matching(&reserve.calls(), "systemctl --user set-property --runtime agent.slice AllowedCPUs=0-23 MemoryMax=infinity"), 1, "off gives agent.slice every CPU by its full list and lifts the cap");
    assert_eq!(reserve.run("status").status.code(), Some(1), "status exits non-zero after off");
}
