use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

const FAKE_SSH: &str = r#"#!/bin/sh
for a; do last=$a; done
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
    fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/job-run"), root.join("bin/job-run")).unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), root.join("bin/north")).unwrap();
    fs::write(root.join("bin/ssh"), FAKE_SSH).unwrap();
    fs::set_permissions(root.join("bin/ssh"), fs::Permissions::from_mode(0o755)).unwrap();
    scratch
}

fn run(root: &Path, program: &str, arguments: &[&str]) -> i32 {
    let mut command = Command::new(root.join("bin").join(program));
    command
        .args(arguments)
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
    assert_eq!(run(root, "job-run", &["nightly", "--", "sh", "-c", "exit 3"]), 3);
    fs::remove_file(root.join("down")).unwrap();
    fs::remove_file(root.join("ssh.log")).unwrap();
    assert_eq!(run(root, "job-run", &["nightly", "--", "true"]), 0);
    assert_eq!(run(root, "north", &["threads", "list"]), 0);
    let delivered: Vec<String> = ssh_log(root).into_iter().filter(|line| line.contains("--finished")).collect();
    assert_eq!(delivered.len(), 2, "{delivered:?}");
    assert!(delivered.iter().any(|line| line.ends_with("--rc 0")) && delivered.iter().any(|line| line.ends_with("--rc 3")), "{delivered:?}");
    assert!(ssh_log(root).last().is_some_and(|line| line.ends_with("north threads snapshot-pages")), "{:?}", ssh_log(root));
}

#[test]
fn job_run_starts_its_job_and_returns_with_it_while_the_start_record_stalls() {
    let scratch = scratch();
    let root = scratch.path();
    fs::write(root.join("stall-start"), "").unwrap();
    let begun = Instant::now();
    assert_eq!(run(root, "job-run", &["fleet-relay", "--", "sh", "-c", "sleep 1; exit 4"]), 4);
    assert!(begun.elapsed() < Duration::from_secs(6), "{:?}", begun.elapsed());
    assert!(ssh_log(root).iter().any(|line| line.contains("job-run fleet-relay") && line.ends_with("--rc 4")), "{:?}", ssh_log(root));
}

fn tool(name: &str) -> std::path::PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{name} not on PATH"))
}

#[test]
fn job_run_records_the_finish_when_the_callers_path_has_no_bash() {
    let scratch = scratch();
    let root = scratch.path();
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for name in ["env", "timeout", "dirname", "readlink", "hostname", "id", "cat", "true"] {
        symlink(tool(name), tools.join(name)).unwrap();
    }
    fs::remove_file(root.join("bin/north")).unwrap();
    fs::write(
        root.join("bin/north"),
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"$HOME/north.log\"\n",
    )
    .unwrap();
    fs::set_permissions(root.join("bin/north"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(tool("bash"));
    command.arg(root.join("bin/job-run")).args(["checks", "--", "true"]).env("HOME", root).env("PATH", &tools);
    let mut child = {
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    let log = fs::read_to_string(root.join("north.log")).unwrap_or_default();
    assert!(log.lines().any(|line| line.contains("job-run checks") && line.contains("--finished") && line.ends_with("--rc 0")), "{log:?}");
}

#[test]
fn job_run_notify_returns_before_its_detached_job_ends_and_sends_the_jobs_exit_to_the_starting_session_without_bash_on_path() {
    let scratch = scratch();
    let root = scratch.path();
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for name in ["env", "timeout", "dirname", "readlink", "hostname", "id", "cat", "mkdir", "setsid", "sh", "sleep", "flock", "rm"] {
        symlink(tool(name), tools.join(name)).unwrap();
    }
    fs::remove_file(root.join("bin/north")).unwrap();
    fs::write(root.join("bin/north"), "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"$HOME/north.log\"\n").unwrap();
    fs::set_permissions(root.join("bin/north"), fs::Permissions::from_mode(0o755)).unwrap();
    let release = root.join("release");
    let job = format!("until [ -f {} ]; do sleep 0.1; done; echo \"finished with ${{JOB_RUN_LOG:-no log}} set\"; exit 3", release.display());
    let mut command = Command::new(tool("bash"));
    command
        .arg(root.join("bin/job-run"))
        .args(["--notify", "soak", "--", "sh", "-c", &job])
        .env("HOME", root)
        .env_remove("XDG_STATE_HOME")
        .env("NORTH_FLEET_SESSION", "c-test")
        .env("PATH", &tools)
        .stdout(std::process::Stdio::piped());
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
    let jobs = root.join(".local/state/north/jobs");
    let begun = Instant::now();
    let record = loop {
        let found = fs::read_dir(&jobs).into_iter().flatten().flatten().map(|entry| entry.path()).find(|path| path.extension().is_some_and(|extension| extension == "job"));
        if let Some(found) = found.filter(|path| fs::metadata(path).is_ok_and(|meta| meta.len() > 0)) {
            break found;
        }
        assert!(begun.elapsed() < Duration::from_secs(10), "the running job never wrote its record");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(fs::read_to_string(&record).unwrap(), "c-test\n");
    let locked = || matches!(fs::File::open(&record).unwrap().try_lock_shared(), Err(fs::TryLockError::WouldBlock));
    let begun = Instant::now();
    while !locked() {
        assert!(begun.elapsed() < Duration::from_secs(10), "the running job never locked its record");
        std::thread::sleep(Duration::from_millis(50));
    }
    fs::write(&release, "").unwrap();
    let log = root.join("north.log");
    let begun = Instant::now();
    let sent = loop {
        let text = fs::read_to_string(&log).unwrap_or_default();
        if let Some(line) = text.lines().find(|line| line.starts_with("fleet send")) {
            break line.to_owned();
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "{text:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(sent.starts_with("fleet send c-test job-run: soak exited 3; output in "), "{sent}");
    let job_log = sent.rsplit(' ').next().unwrap();
    assert!(fs::read_to_string(job_log).unwrap().contains("finished with no log set"), "{job_log}");
    let text = fs::read_to_string(&log).unwrap();
    assert!(text.lines().any(|line| line.contains("job-run soak") && line.ends_with("--rc 3")), "{text:?}");
    let begun = Instant::now();
    while record.exists() {
        assert!(begun.elapsed() < Duration::from_secs(10), "the finished job left its record");
        std::thread::sleep(Duration::from_millis(50));
    }
}
