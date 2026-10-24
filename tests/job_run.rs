use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;
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

fn scratch() -> tempfile::TempDir {
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
    Command::new(root.join("bin").join(program))
        .args(arguments)
        .env("HOME", root)
        .env_remove("THREADS_DB")
        .env_remove("XDG_RUNTIME_DIR")
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .status()
        .unwrap()
        .code()
        .unwrap()
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
    assert!(ssh_log(root).last().is_some_and(|line| line.ends_with("threads snapshot")), "{:?}", ssh_log(root));
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
