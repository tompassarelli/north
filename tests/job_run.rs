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
