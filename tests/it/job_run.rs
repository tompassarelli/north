use north_core::foundation::shim;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use north_core::fleet::store::{RowKind, Spool};

const FAKE_SSH: &str = r#"for a; do last=$a; done
printf '%s\n' "$last" >> "$HOME/ssh.log"
[ -f "$HOME/down" ] && exit 255
case "$last" in
  *--finished*) ;;
  *job-run*) [ -f "$HOME/stall-start" ] && exec sleep 60 ;;
esac
exit 0
"#;

static FORK_GATE: Mutex<()> = Mutex::new(());

fn fork_gate() -> MutexGuard<'static, ()> {
    FORK_GATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn scratch() -> tempfile::TempDir {
    let _gate = fork_gate();
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join(".config/threads")).unwrap();
    fs::write(root.join(".config/threads/remote"), "ledger-host\n").unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), root.join("bin/north")).unwrap();
    shim::write_script(&root.join("bin/ssh"), FAKE_SSH);
    scratch
}

fn run(root: &Path, program: &str, arguments: &[&str]) -> i32 {
    let mut command = Command::new(root.join("bin").join(program));
    command
        .args(arguments)
        .current_dir(root)
        .env("HOME", root)
        .env_remove("THREADS_DB")
        .env_remove("XDG_RUNTIME_DIR")
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()));
    let mut child = {
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    child.wait().unwrap().code().unwrap()
}

fn ssh_log(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("ssh.log")).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn job_run_keeps_the_exit_code_and_spools_the_finish_while_the_ledger_host_is_down_until_the_next_flush() {
    let scratch = scratch();
    let root = scratch.path();
    fs::write(root.join("down"), "").unwrap();
    assert_eq!(run(root, "north", &["job-run", "nightly", "--", "sh", "-c", "exit 3"]), 3);
    fs::remove_file(root.join("down")).unwrap();
    fs::remove_file(root.join("ssh.log")).unwrap();
    assert_eq!(run(root, "north", &["job-run", "nightly", "--", "true"]), 0);
    assert_eq!(run(root, "north", &["threads", "list"]), 0);
    let delivered: Vec<String> = ssh_log(root).into_iter().filter(|line| line.contains("--finished")).collect();
    assert_eq!(delivered.len(), 2, "{delivered:?}");
    assert!(delivered.iter().any(|line| line.ends_with("--rc 0")) && delivered.iter().any(|line| line.ends_with("--rc 3")), "{delivered:?}");
    let listed = ssh_log(root).iter().position(|line| line.ends_with("north threads list")).expect("the read reached the ledger host");
    assert_eq!(ssh_log(root)[..listed].iter().filter(|line| line.contains("--finished")).count(), 2, "{:?}", ssh_log(root));
    let begun = Instant::now();
    while !ssh_log(root)[listed..].iter().any(|line| line.ends_with("north threads snapshot-pages")) {
        assert!(begun.elapsed() < Duration::from_secs(10), "the stale snapshot was never pulled: {:?}", ssh_log(root));
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn job_run_starts_its_job_and_returns_with_it_while_the_start_record_stalls() {
    let scratch = scratch();
    let root = scratch.path();
    fs::write(root.join("stall-start"), "").unwrap();
    let begun = Instant::now();
    assert_eq!(run(root, "north", &["job-run", "fleet-tick", "--", "sh", "-c", "sleep 1; exit 4"]), 4);
    assert!(begun.elapsed() < Duration::from_secs(6), "{:?}", begun.elapsed());
    assert!(ssh_log(root).iter().any(|line| line.contains("job-run fleet-tick") && line.ends_with("--rc 4")), "{:?}", ssh_log(root));
}

fn tool(name: &str) -> std::path::PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{name} not on PATH"))
}

fn tools_without_bash(root: &Path, names: &[&str]) -> std::path::PathBuf {
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for name in names {
        symlink(tool(name), tools.join(name)).unwrap();
    }
    symlink(root.join("bin/ssh"), tools.join("ssh")).unwrap();
    tools
}

fn north_job_run(root: &Path, tools: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command.arg("job-run").args(arguments).current_dir(root).env("HOME", root).env_remove("XDG_STATE_HOME").env_remove("THREADS_DB").env_remove("XDG_RUNTIME_DIR").env("PATH", tools);
    command
}

fn spooled(root: &Path) -> Vec<north_core::fleet::store::FleetRow> {
    Spool::at(&root.join(".local/state/north/fleet")).pending().unwrap()
}

fn await_exit(root: &Path) -> Vec<north_core::fleet::store::FleetRow> {
    let begun = Instant::now();
    loop {
        let rows = spooled(root);
        if rows.iter().any(|row| row.kind == RowKind::JobExit) {
            return rows;
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "the job never sent its exit: {rows:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn job_run_records_the_finish_when_the_callers_path_has_no_bash() {
    let scratch = scratch();
    let root = scratch.path();
    let tools = tools_without_bash(root, &["timeout", "true"]);
    let status = {
        let mut command = north_job_run(root, &tools, &["checks", "--", "true"]);
        let _gate = fork_gate();
        command.spawn().unwrap().wait().unwrap()
    };
    assert_eq!(status.code(), Some(0));
    assert!(ssh_log(root).iter().any(|line| line.contains("job-run checks") && line.contains("--finished") && line.ends_with("--rc 0")), "{:?}", ssh_log(root));
}

#[test]
fn job_run_notify_returns_before_its_detached_job_ends_and_sends_the_jobs_exit_to_the_starting_session_without_bash_on_path() {
    let scratch = scratch();
    let root = scratch.path();
    let tools = tools_without_bash(root, &["timeout", "setsid", "sh", "sleep"]);
    let release = root.join("release");
    let job = format!("until [ -f {} ]; do sleep 0.1; done; echo \"finished with ${{JOB_RUN_LOG:-no log}} set\"; exit 3", release.display());
    let mut command = north_job_run(root, &tools, &["--notify", "soak", "--", "sh", "-c", &job]);
    command.env("NORTH_FLEET_SESSION", "c-test").stdout(std::process::Stdio::piped());
    let mut child = {
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    let begun = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if begun.elapsed() > Duration::from_secs(10) {
            child.kill().unwrap();
            panic!("job-run --notify did not return before its job ended");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut said = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut said).unwrap();
    assert_eq!(status.code(), Some(0), "{said}");
    assert!(said.contains("fleet message to c-test"), "{said}");
    fs::write(&release, "").unwrap();
    let rows = await_exit(root);
    let exit = rows.last().unwrap();
    assert!(exit.to == "c-test" && exit.job.as_ref().is_some_and(|job| job.key.starts_with("soak@") && job.exit == Some(3)), "{exit:?}");
    assert!(exit.text.starts_with("job-run: soak exited 3; output in "), "{}", exit.text);
    let job_log = exit.text.rsplit(' ').next().unwrap();
    assert!(fs::read_to_string(job_log).unwrap().contains("finished with no log set"), "{job_log}");
    assert!(rows.iter().any(|row| row.kind == RowKind::JobStart && row.text.starts_with("job-run: soak started; output in ")), "{rows:?}");
    assert!(ssh_log(root).iter().any(|line| line.contains("job-run soak") && line.ends_with("--rc 3")), "{:?}", ssh_log(root));
}

#[test]
fn job_run_notify_runs_its_job_in_its_own_systemd_scope_so_the_callers_unit_stopping_does_not_kill_it() {
    let scratch = scratch();
    let root = scratch.path();
    let tools = tools_without_bash(root, &["timeout", "setsid", "sh", "sleep", "true"]);
    shim::write_script(&tools.join("systemd-run"), "printf '%s\\n' \"$*\" >> \"$HOME/systemd-run.log\"\nwhile [ \"${1#--}\" != \"$1\" ]; do shift; done\nexec \"$@\"\n");
    let status = {
        let mut command = north_job_run(root, &tools, &["--notify", "scoped", "--", "true"]);
        command.env("NORTH_FLEET_SESSION", "c-test");
        let _gate = fork_gate();
        command.spawn().unwrap().wait().unwrap()
    };
    assert_eq!(status.code(), Some(0));
    let rows = await_exit(root);
    assert!(rows.last().unwrap().text.starts_with("job-run: scoped exited 0"), "{rows:?}");
    let scopes = fs::read_to_string(root.join("systemd-run.log")).unwrap();
    assert!(scopes.lines().any(|line| line.starts_with("--user --scope --quiet --collect ") && line.contains(" job-run --notify scoped -- true") && line.contains("--slice=agent.slice")), "{scopes}");
}

#[test]
fn job_run_notify_spools_a_start_with_a_three_minute_lease_a_beat_each_interval_and_the_exit_with_its_code_each_addressed_to_the_session() {
    let scratch = scratch();
    let root = scratch.path();
    let mut command = Command::new(root.join("bin/north"));
    command
        .current_dir(root)
        .args(["job-run", "--notify", "soak", "--", "sh", "-c", "sleep 3.5; exit 3"])
        .env("HOME", root)
        .env_remove("XDG_STATE_HOME")
        .env_remove("THREADS_DB")
        .env_remove("XDG_RUNTIME_DIR")
        .env("JOB_RUN_BEAT_SECS", "1")
        .env("NORTH_FLEET_SESSION", "c-test")
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()));
    let mut child = {
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let spool = Spool::at(&root.join(".local/state/north/fleet"));
    let begun = Instant::now();
    let rows = loop {
        let rows = spool.pending().unwrap();
        if rows.iter().any(|row| row.kind == RowKind::JobExit) {
            break rows;
        }
        assert!(begun.elapsed() < Duration::from_secs(30), "{rows:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    let kinds: Vec<RowKind> = rows.iter().map(|row| row.kind).collect();
    assert_eq!(kinds.first(), Some(&RowKind::JobStart), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&RowKind::JobExit), "{kinds:?}");
    let beats = kinds.iter().filter(|kind| **kind == RowKind::JobBeat).count();
    assert!((2..=4).contains(&beats), "{kinds:?}");
    assert!(rows.iter().all(|row| row.to == "c-test" && row.job.as_ref().is_some_and(|job| job.key.starts_with("soak@"))), "{rows:?}");
    let key = &rows[0].job.as_ref().unwrap().key;
    assert!(rows.iter().all(|row| row.job.as_ref().is_some_and(|job| &job.key == key)), "{rows:?}");
    assert_eq!(rows[0].job.as_ref().and_then(|job| job.lease_secs), Some(180));
    assert!(rows.iter().filter(|row| row.kind == RowKind::JobBeat).all(|row| row.job.as_ref().and_then(|job| job.lease_secs) == Some(180)));
    let exit = rows.last().unwrap();
    assert_eq!(exit.job.as_ref().and_then(|job| job.exit), Some(3));
    assert!(exit.text.starts_with("job-run: soak exited 3; output in "), "{}", exit.text);
}

fn zombie_children(parent: u32) -> Vec<String> {
    fs::read_dir("/proc")
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path().join("stat")).ok())
        .filter_map(|stat| {
            let (before, after) = stat.rsplit_once(')')?;
            let mut fields = after.split_whitespace();
            let (state, ppid) = (fields.next()?, fields.next()?);
            (state == "Z" && ppid == parent.to_string()).then(|| before.to_owned())
        })
        .collect()
}

#[test]
fn job_run_reaps_its_start_record_that_exits_while_the_job_still_runs() {
    let scratch = scratch();
    let root = scratch.path();
    let mut job = {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .args(["job-run", "slow", "--", "sleep", "4"])
            .current_dir(root)
            .env("HOME", root)
            .env_remove("THREADS_DB")
            .env_remove("XDG_RUNTIME_DIR")
            .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()));
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    let begun = Instant::now();
    while !ssh_log(root).iter().any(|line| line.contains("job-run slow")) {
        assert!(begun.elapsed() < Duration::from_secs(3), "the start record never ran");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(500));
    let zombies = zombie_children(job.id());
    let code = job.wait().unwrap().code();
    assert_eq!(zombies, Vec::<String>::new());
    assert_eq!(code, Some(0));
}
