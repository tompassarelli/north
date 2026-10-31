use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use north_core::fleet::store::{RowKind, Spool};

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
    for name in ["env", "timeout", "dirname", "readlink", "hostname", "id", "cat", "mkdir", "setsid", "sh", "sleep", "rm"] {
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
    fs::write(&release, "").unwrap();
    let log = root.join("north.log");
    let begun = Instant::now();
    let sent = loop {
        let text = fs::read_to_string(&log).unwrap_or_default();
        if let Some(line) = text.lines().find(|line| line.starts_with("fleet spool job exit")) {
            break line.to_owned();
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "{text:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(sent.starts_with("fleet spool job exit c-test soak@") && sent.contains(" 3 job-run: soak exited 3; output in "), "{sent}");
    let job_log = sent.rsplit(' ').next().unwrap();
    assert!(fs::read_to_string(job_log).unwrap().contains("finished with no log set"), "{job_log}");
    let text = fs::read_to_string(&log).unwrap();
    assert!(text.lines().any(|line| line.starts_with("fleet spool job start c-test soak@") && line.contains(" job-run: soak started; output in ")), "{text:?}");
    assert!(text.lines().any(|line| line.starts_with("fleet spool job beats c-test soak@") && line.contains(" 60 ")), "{text:?}");
    assert!(text.lines().any(|line| line.contains("job-run soak") && line.ends_with("--rc 3")), "{text:?}");
}

#[test]
fn job_run_notify_runs_its_job_in_its_own_systemd_scope_so_the_callers_unit_stopping_does_not_kill_it() {
    let scratch = scratch();
    let root = scratch.path();
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for name in ["env", "timeout", "dirname", "readlink", "hostname", "id", "cat", "mkdir", "setsid", "sh", "sleep", "rm", "true"] {
        symlink(tool(name), tools.join(name)).unwrap();
    }
    fs::write(tools.join("systemd-run"), "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/systemd-run.log\"\nwhile [ \"${1#--}\" != \"$1\" ]; do shift; done\nexec \"$@\"\n").unwrap();
    fs::set_permissions(tools.join("systemd-run"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(root.join("bin/north")).unwrap();
    fs::write(root.join("bin/north"), "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"$HOME/north.log\"\n").unwrap();
    fs::set_permissions(root.join("bin/north"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(tool("bash"));
    command.arg(root.join("bin/job-run")).args(["--notify", "scoped", "--", "true"]).env("HOME", root).env_remove("XDG_STATE_HOME").env("NORTH_FLEET_SESSION", "c-test").env("PATH", &tools);
    let mut child = {
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let begun = Instant::now();
    while !fs::read_to_string(root.join("north.log")).unwrap_or_default().contains("job-run: scoped exited 0") {
        assert!(begun.elapsed() < Duration::from_secs(20), "the scoped job never sent its exit");
        std::thread::sleep(Duration::from_millis(100));
    }
    let scopes = fs::read_to_string(root.join("systemd-run.log")).unwrap();
    assert!(scopes.lines().any(|line| line.starts_with("--user --scope --quiet --collect ") && line.contains("bin/job-run --notify scoped -- true")), "{scopes}");
}

#[test]
fn job_run_notify_spools_a_start_with_a_three_minute_lease_a_beat_each_interval_and_the_exit_with_its_code_each_addressed_to_the_session() {
    let scratch = scratch();
    let root = scratch.path();
    let mut command = Command::new(root.join("bin/job-run"));
    command
        .args(["--notify", "soak", "--", "sh", "-c", "sleep 3.5; exit 3"])
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
