use super::*;

const OLD: &str = "1111111111111111111111111111111111111111";
const LIVE: &str = "2222222222222222222222222222222222222222";
const FAKE_UPTIME_SECS: u64 = 1_000_000;

fn north(runtime: &str) -> String {
    format!("/home/tom/.local/state/north/runtimes/{runtime}/tree/bin/../../north")
}

struct FakeProc(tempfile::TempDir, u32);

impl FakeProc {
    fn new() -> Self {
        let proc = Self(tempfile::tempdir().unwrap(), 100);
        std::fs::write(proc.0.path().join("loadavg"), "0.50 0.50 0.50 1/100 1\n").unwrap();
        std::fs::write(proc.0.path().join("uptime"), format!("{FAKE_UPTIME_SECS}.00 0.00\n")).unwrap();
        proc
    }

    fn spawn(&mut self, comm: &str, argv: &[&str]) -> u32 {
        self.spawn_under(1, comm, argv)
    }

    fn spawn_under(&mut self, parent: u32, comm: &str, argv: &[&str]) -> u32 {
        self.spawn_aged(parent, comm, argv, 0)
    }

    fn spawn_aged(&mut self, parent: u32, comm: &str, argv: &[&str], age_secs: u64) -> u32 {
        self.spawn_in_aged(parent, "", comm, argv, age_secs)
    }

    fn spawn_in(&mut self, unit: &str, comm: &str, argv: &[&str]) -> u32 {
        self.spawn_in_aged(1, unit, comm, argv, 0)
    }

    fn spawn_in_aged(&mut self, parent: u32, unit: &str, comm: &str, argv: &[&str], age_secs: u64) -> u32 {
        self.1 += 1;
        let pid = self.0.path().join(self.1.to_string());
        std::fs::create_dir(&pid).unwrap();
        std::fs::write(pid.join("comm"), format!("{comm}\n")).unwrap();
        std::fs::write(pid.join("stat"), format!("{} ({comm}) S {parent}{} {}\n", self.1, " 0".repeat(17), (FAKE_UPTIME_SECS - age_secs) * 100)).unwrap();
        std::fs::write(pid.join("cmdline"), argv.iter().map(|word| format!("{word}\0")).collect::<String>()).unwrap();
        std::fs::write(pid.join("cgroup"), format!("0::/user.slice/agent.slice/{unit}\n")).unwrap();
        self.1
    }

    fn sessions(&mut self, live: usize, hosts_each: usize) {
        let north = north(LIVE);
        for session in 0..live {
            let name = format!("worker-{session}");
            let unit = unit_of(&name);
            for _ in 0..hosts_each {
                self.spawn("north", &[&north, "session-host", "--name", &name, "--generation", "1", "--", &north, "session-link", "--ssh", "gex45"]);
            }
            self.spawn("north", &[&north, "session-link", "--ssh", "gex45", "--unit", &name]);
            self.spawn("north", &[&north, "fleet", "watchdog", &name]);
            self.hold(&unit, false);
        }
    }

    fn hold(&self, unit: &str, ended: bool) {
        if !ended {
            std::fs::create_dir_all(self.0.path().join("cgroup").join(unit)).unwrap();
        }
    }

    fn read(&self) -> NorthProcesses {
        let output = Command::new("bash")
            .arg("-c")
            .arg(facts_script())
            .env("NORTH_SETTLE_PROC", self.0.path())
            .env("NORTH_SETTLE_CGROUP", self.0.path().join("cgroup"))
            .env("HOME", self.0.path())
            .env("XDG_STATE_HOME", self.0.path())
            .output()
            .unwrap();
        let read = sections(&String::from_utf8_lossy(&output.stdout));
        NorthProcesses::read(&read["north"], &read["held"], Some(LIVE), default_number("north_process_leak_window_sec") as u64)
    }

    fn problems(&self) -> (Vec<String>, Value) {
        let north = self.read();
        (north.problems(), north.record())
    }
}

fn unit_of(session: &str) -> String {
    north_base::platform::session_scope::session_unit("claude", &format!("{session}-00000000-0000-4000-8000-000000000000"))
}

#[test]
fn only_processes_named_north_count_and_a_multi_line_argument_stays_one_process() {
    let mut proc = FakeProc::new();
    proc.sessions(2, 1);
    let north = north(LIVE);
    proc.spawn("north", &[&north, "session-host", "--name", "worker-2", "--", "claude", "--append-system-prompt", "first\n\tsecond\nthird"]);
    proc.spawn("claude", &["claude", "--cwd", "/home/tom/code/north/worktrees/a", "--append-system-prompt", "north\nsession-host\nfleet watchdog"]);
    proc.spawn("bash", &["bash", "/home/tom/code/north/worktrees/a/scripts/check.sh", "fleet", "list"]);
    proc.spawn("rustc", &["rustc", "--crate-name", "north_core", "/home/tom/code/north/main/crates/north-core/src/lib.rs"]);
    let (problems, record) = proc.problems();
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(record, json!({"transient": 0, "outlasting": 0, "liveSessions": 3, "residents": {"session-host": 3, "session-link": 2, "fleet watchdog": 2, "land serve": 0}, "strayWatchdogs": 0}));
}

#[test]
fn a_session_host_a_land_check_test_runs_from_its_sandbox_build_is_not_a_fleet_resident() {
    let mut proc = FakeProc::new();
    proc.sessions(1, 1);
    proc.spawn("north", &["/home/tom/code/north/sandbox/target/debug/north", "session-host", "--name", "worker", "--generation", "1", "--state-dir", "/tmp/.tmpOQQkoY/.local/state/north/fleet/worker"]);
    proc.spawn("north", &["/home/tom/code/north/sandbox/target/debug/north", "session-host", "--name", "worker-0", "--generation", "1"]);
    let (problems, record) = proc.problems();
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(record["residents"]["session-host"], json!(1));
}

#[test]
fn old_and_ended_session_watchdogs_after_a_runtime_swap_are_no_fault_while_two_live_ones_for_one_session_are_a_leak() {
    let mut proc = FakeProc::new();
    proc.sessions(2, 1);
    let old = north(OLD);
    proc.spawn("north", &[&old, "fleet", "watchdog", "worker-0"]);
    proc.spawn("north", &[&old, "fleet", "watchdog", "worker-1"]);
    proc.spawn("north", &[&north(LIVE), "fleet", "watchdog", "worker-ended"]);
    let (problems, record) = proc.problems();
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(record["residents"]["fleet watchdog"], json!(2));
    assert_eq!(record["strayWatchdogs"], json!(3));

    let mut proc = FakeProc::new();
    proc.sessions(2, 1);
    proc.spawn("north", &[&north(LIVE), "fleet", "watchdog", "worker-1"]);
    let (problems, _) = proc.problems();
    assert_eq!(problems, vec!["2 fleet watchdog processes of the live runtime for live session worker-1".to_owned()]);
}

#[test]
fn the_unit_running_one_land_serve_reads_as_one_and_a_second_unit_for_a_repository_pages() {
    let mut proc = FakeProc::new();
    let (north, old) = (north(LIVE), north(OLD));
    const REST: &str = "north-land-rest.service";
    let rest = proc.spawn_in(REST, "north", &[&north, "land", "serve", "--all"]);
    proc.spawn_in_aged(rest, REST, "north", &[&north, "land", "serve", "--all"], 0);
    for check in 0..100 {
        proc.spawn_in_aged(rest, REST, "north", &[&north, "capacity", "offload", "run", "--host", "gex45", "--head", &check.to_string()], 0);
    }
    for repo in ["north", "south", "wisp", "makgora", "battlecraft", "nixos-config", "padbridge", "glide", "clause", "slippi-launcher", "makgora-client"] {
        proc.spawn_in(&format!("north-land@{repo}.service"), "north", &[&north, "land", "serve", "--repo", &format!("/home/tom/code/{repo}/main")]);
    }
    proc.spawn("north", &[&north, "land", "serve", "--all"]);
    proc.spawn_in("run-p1234-i1235.scope", "north", &["/home/tom/code/north/sandbox/target/debug/north", "land", "serve", "--once", "--repo", "/tmp/.tmpGzR5Ue/proj/main"]);
    let (problems, record) = proc.problems();
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!((record["transient"].clone(), record["residents"]["land serve"].clone()), (json!(102), json!(12)));
    proc.spawn_in("north-land@wisp-old.service", "north", &[&old, "land", "serve", "--repo", "/home/tom/code/wisp/main"]);
    assert_eq!(proc.problems().0, ["2 land serve units for /home/tom/code/wisp/main: north-land@wisp-old.service, north-land@wisp.service"]);
}

const TIMER: (&str, &str, &str, &str) = ("north-runtime-follow.timer", "active", "waiting", "success");

fn unit_problems(watchdog: (&str, &str, &str), age_secs: u64) -> Vec<String> {
    const UPTIME_SECS: u64 = 1_000_000;
    let dir = tempfile::tempdir().unwrap();
    let systemctl = dir.path().join("systemctl");
    crate::foundation::shim::write_executable(&systemctl, "#!/usr/bin/env bash\nunit=\"${@: -1}\"\ncase \"$2\" in\nis-active) cat \"$FAKE_UNITS/$unit.active\"; exit 3;;\nshow) cat \"$FAKE_UNITS/$unit.$4\";;\nesac\n");
    for (unit, active, sub, result) in [("capacity-watchdog.service", watchdog.0, watchdog.1, watchdog.2), TIMER] {
        std::fs::write(dir.path().join(format!("{unit}.active")), format!("{active}\n")).unwrap();
        std::fs::write(dir.path().join(format!("{unit}.SubState")), format!("{sub}\n")).unwrap();
        std::fs::write(dir.path().join(format!("{unit}.Result")), format!("{result}\n")).unwrap();
        std::fs::write(dir.path().join(format!("{unit}.StateChangeTimestampMonotonic")), format!("{}\n", (UPTIME_SECS - age_secs) * 1_000_000)).unwrap();
        std::fs::write(dir.path().join(format!("{unit}.TimeoutStartUSec")), "1min 30s\n").unwrap();
    }
    std::fs::write(dir.path().join("uptime"), format!("{UPTIME_SECS}.00 0.00\n")).unwrap();
    let output = Command::new("bash")
        .arg("-c")
        .arg(facts_script())
        .env("PATH", format!("{}:{}", dir.path().display(), std::env::var("PATH").unwrap()))
        .env("FAKE_UNITS", dir.path())
        .env("NORTH_SETTLE_PROC", dir.path())
        .env("HOME", dir.path())
        .env("XDG_STATE_HOME", dir.path())
        .output()
        .unwrap();
    let units = sections(&String::from_utf8_lossy(&output.stdout)).remove("units").unwrap();
    let facts = Facts {
        host: "gex45".to_owned(),
        runtime: Some(LIVE.to_owned()),
        units: UNITS.iter().zip(units.lines()).map(|(unit, state)| ((*unit).to_owned(), state.to_owned())).collect(),
        settings: vec!["settings.json".to_owned()],
        north: Some(NorthProcesses::default()),
        ..Facts::default()
    };
    problems(&facts, |_| true)
}

#[test]
fn a_watchdog_restarting_cleanly_or_starting_inside_its_timeout_is_healthy_while_a_crash_or_stuck_start_is_a_fault() {
    assert_eq!(unit_problems(("activating", "auto-restart", "success"), 30), Vec::<String>::new());
    assert_eq!(unit_problems(("activating", "start", "success"), 30), Vec::<String>::new());
    assert_eq!(unit_problems(("activating", "start", "success"), 120), vec!["capacity-watchdog.service is activating start success".to_owned()]);
    assert_eq!(unit_problems(("activating", "auto-restart", "exit-code"), 30), vec!["capacity-watchdog.service is activating auto-restart exit-code".to_owned()]);
    assert_eq!(unit_problems(("failed", "failed", "exit-code"), 30), vec!["capacity-watchdog.service is failed".to_owned()]);
}

#[test]
fn a_remote_job_counts_once_with_its_wrapper_chain_and_orphaned_wrappers_still_count() {
    let mut proc = FakeProc::new();
    proc.sessions(2, 1);
    let north = north(LIVE);
    proc.spawn_in("north-land@north.service", "north", &[&north, "land", "serve", "--repo", "/home/tom/code/north/main"]);
    for job in 0..65 {
        let check = proc.spawn("north", &[&north, "land", "checks", "--base", "b", "--head", &job.to_string()]);
        let deadline = proc.spawn_under(check, "north", &[&north, "capacity", "deadline", "-", "3600", "--", &north, "capacity", "offload", "run"]);
        proc.spawn_under(deadline, "north", &[&north, "capacity", "offload", "run", "--host", "gex45"]);
    }
    let (problems, record) = proc.problems();
    assert!(problems.is_empty(), "on 2026-11-08 the laptop read 197 transient north processes, three per offloaded land check, so settle refused it 8 times and the lander runtime stopped following main: {problems:?}");
    assert_eq!(record["transient"], json!(65));
    for orphan in 0..36 {
        proc.spawn("north", &[&north, "capacity", "offload", "run", "--host", "gex45", "--head", &orphan.to_string()]);
    }
    assert_eq!(proc.problems().1["transient"], json!(101));
}

#[test]
fn a_tool_server_in_a_held_unit_whose_cli_runs_is_growth_and_one_in_a_unit_whose_cli_ended_is_a_leak() {
    let mut proc = FakeProc::new();
    proc.sessions(2, 1);
    let tool = north(LIVE);
    proc.spawn_in(&unit_of("worker-0"), "north", &[&tool, "--tool-server", "hand-in", "finish"]);
    proc.spawn_in(&unit_of("worker-1"), "north", &[&tool, "--tool-server", "hand-in", "finish"]);
    let read = proc.read();
    assert_eq!(read.problems(), Vec::<String>::new());
    assert_eq!(read.growth(), "transient growth: 2 for 2 live sessions");
    assert_eq!(read.leak(None), "transient leak: none");
    let unit = unit_of("worker-ended");
    proc.hold(&unit, true);
    proc.spawn_in(&unit, "north", &[&tool, "--tool-server", "hand-in", "finish"]);
    let read = proc.read();
    let message = format!("1 transient north processes outlived their ended session {unit}");
    assert_eq!(read.problems(), vec![message.clone()]);
    assert_eq!(read.leak(None), format!("transient leak: {message}"));
    assert_eq!(read.growth(), "transient growth: 2 for 2 live sessions");
}

#[test]
fn an_empty_unit_state_is_unread_and_pages_no_one() {
    let facts = Facts {
        host: "gex45".to_owned(),
        runtime: Some(LIVE.to_owned()),
        units: UNITS.iter().map(|unit| ((*unit).to_owned(), String::new())).collect(),
        settings: vec!["settings.json".to_owned()],
        north: Some(NorthProcesses::default()),
        ..Facts::default()
    };
    assert_eq!(problems(&facts, |_| true), Vec::<String>::new());
}
