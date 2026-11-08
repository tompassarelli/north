use super::*;

const OLD: &str = "1111111111111111111111111111111111111111";
const LIVE: &str = "2222222222222222222222222222222222222222";

fn north(runtime: &str) -> String {
    format!("/home/tom/.local/state/north/runtimes/{runtime}/tree/bin/../../north")
}

struct FakeProc(tempfile::TempDir, u32);

impl FakeProc {
    fn new() -> Self {
        let proc = Self(tempfile::tempdir().unwrap(), 100);
        std::fs::write(proc.0.path().join("loadavg"), "0.50 0.50 0.50 1/100 1\n").unwrap();
        proc
    }

    fn spawn(&mut self, comm: &str, argv: &[&str]) -> u32 {
        self.spawn_under(1, comm, argv)
    }

    fn spawn_under(&mut self, parent: u32, comm: &str, argv: &[&str]) -> u32 {
        self.1 += 1;
        let pid = self.0.path().join(self.1.to_string());
        std::fs::create_dir(&pid).unwrap();
        std::fs::write(pid.join("comm"), format!("{comm}\n")).unwrap();
        std::fs::write(pid.join("stat"), format!("{} ({comm}) S {parent} 0 0\n", self.1)).unwrap();
        std::fs::write(pid.join("cmdline"), argv.iter().map(|word| format!("{word}\0")).collect::<String>()).unwrap();
        self.1
    }

    fn sessions(&mut self, live: usize, hosts_each: usize) {
        let north = north(LIVE);
        for session in 0..live {
            let name = format!("worker-{session}");
            for _ in 0..hosts_each {
                self.spawn("north", &[&north, "session-host", "--name", &name, "--generation", "1", "--", &north, "session-link", "--ssh", "gex45"]);
            }
            self.spawn("north", &[&north, "session-link", "--ssh", "gex45", "--unit", &name]);
            self.spawn("north", &[&north, "fleet", "watchdog", &name]);
        }
    }

    fn problems(&self) -> (Vec<String>, Value) {
        let output = Command::new("bash")
            .arg("-c")
            .arg(facts_script())
            .env("NORTH_SETTLE_PROC", self.0.path())
            .env("HOME", self.0.path())
            .env("XDG_STATE_HOME", self.0.path())
            .output()
            .unwrap();
        let north = NorthProcesses::read(&sections(&String::from_utf8_lossy(&output.stdout))["north"], Some(LIVE));
        (north.problems(), north.record())
    }
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
    assert_eq!(record, json!({"transient": 0, "liveSessions": 3, "residents": {"session-host": 3, "session-link": 2, "fleet watchdog": 2, "land serve": 0}, "strayWatchdogs": 0}));
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
fn a_runtime_swap_leaves_old_and_ended_session_watchdogs_that_are_not_a_fault() {
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
}

#[test]
fn two_live_runtime_watchdogs_for_one_live_session_are_a_leak() {
    let mut proc = FakeProc::new();
    proc.sessions(2, 1);
    proc.spawn("north", &[&north(LIVE), "fleet", "watchdog", "worker-1"]);
    let (problems, _) = proc.problems();
    assert_eq!(problems, vec!["2 fleet watchdog processes of the live runtime for live session worker-1".to_owned()]);
}

#[test]
fn the_lander_of_each_repository_is_a_resident_and_a_second_one_for_a_repository_is_the_leak() {
    let mut proc = FakeProc::new();
    let (north, old) = (north(LIVE), north(OLD));
    for check in 0..100 {
        proc.spawn("north", &[&north, "capacity", "offload", "run", "--host", "gex45", "--head", &check.to_string()]);
    }
    proc.spawn("north", &[&north, "land", "serve", "--all"]);
    for repo in ["north", "south", "wisp", "makgora", "battlecraft", "nixos-config", "padbridge", "glide", "clause", "slippi-launcher", "makgora-client"] {
        proc.spawn("north", &[&north, "land", "serve", "--repo", &format!("/home/tom/code/{repo}/main")]);
    }
    proc.spawn("north", &["/home/tom/code/north/sandbox/target/debug/north", "land", "serve", "--once", "--repo", "/tmp/.tmpGzR5Ue/proj/main"]);
    let (problems, record) = proc.problems();
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!((record["transient"].clone(), record["residents"]["land serve"].clone()), (json!(101), json!(12)));
    proc.spawn("north", &[&old, "land", "serve", "--repo", "/home/tom/code/wisp/main"]);
    assert_eq!(proc.problems().0, ["2 land serve processes for /home/tom/code/wisp/main"]);
}

const TIMER: (&str, &str, &str, &str) = ("north-runtime-follow.timer", "active", "waiting", "success");

fn unit_problems(watchdog: (&str, &str, &str)) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let systemctl = dir.path().join("systemctl");
    std::fs::write(&systemctl, "#!/usr/bin/env bash\nunit=\"${@: -1}\"\ncase \"$2\" in\nis-active) cat \"$FAKE_UNITS/$unit.active\"; exit 3;;\nshow) cat \"$FAKE_UNITS/$unit.$4\";;\nesac\n").unwrap();
    std::fs::set_permissions(&systemctl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    for (unit, active, sub, result) in [("capacity-watchdog.service", watchdog.0, watchdog.1, watchdog.2), TIMER] {
        std::fs::write(dir.path().join(format!("{unit}.active")), format!("{active}\n")).unwrap();
        std::fs::write(dir.path().join(format!("{unit}.SubState")), format!("{sub}\n")).unwrap();
        std::fs::write(dir.path().join(format!("{unit}.Result")), format!("{result}\n")).unwrap();
    }
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
fn a_watchdog_waiting_to_restart_after_the_promoted_runtime_moved_is_healthy() {
    assert_eq!(unit_problems(("activating", "auto-restart", "success")), Vec::<String>::new());
}

#[test]
fn a_watchdog_waiting_to_restart_after_a_crash_or_stopped_is_a_fault() {
    assert_eq!(unit_problems(("activating", "auto-restart", "exit-code")), vec!["capacity-watchdog.service is activating auto-restart exit-code".to_owned()]);
    assert_eq!(unit_problems(("activating", "start", "success")), vec!["capacity-watchdog.service is activating start success".to_owned()]);
    assert_eq!(unit_problems(("failed", "failed", "exit-code")), vec!["capacity-watchdog.service is failed".to_owned()]);
}

#[test]
fn a_remote_job_counts_once_with_its_wrapper_chain_and_orphaned_wrappers_still_count() {
    let mut proc = FakeProc::new();
    proc.sessions(2, 1);
    let north = north(LIVE);
    proc.spawn("north", &[&north, "land", "serve", "--repo", "/home/tom/code/north/main"]);
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
