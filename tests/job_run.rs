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
    assert_eq!(run(root, "north", &["job-run", "nightly", "--", "sh", "-c", "exit 3"]), 3);
    fs::remove_file(root.join("down")).unwrap();
    fs::remove_file(root.join("ssh.log")).unwrap();
    assert_eq!(run(root, "north", &["job-run", "nightly", "--", "true"]), 0);
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
    assert_eq!(run(root, "north", &["job-run", "fleet-relay", "--", "sh", "-c", "sleep 1; exit 4"]), 4);
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
fn job_run_records_through_the_path_directories_it_is_given_when_the_callers_path_has_no_ssh() {
    let scratch = scratch();
    let root = scratch.path();
    let empty = root.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command
        .args(["job-run", "--path"])
        .arg(root.join("bin"))
        .args(["checks", "--"])
        .arg(tool("true"))
        .env("HOME", root)
        .env("PATH", &empty);
    let mut child = {
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert!(
        ssh_log(root).iter().any(|line| line.contains("job-run checks") && line.contains("--finished") && line.ends_with("--rc 0")),
        "{:?}",
        ssh_log(root)
    );
}

#[test]
fn job_run_notify_returns_before_its_detached_job_ends_and_posts_the_jobs_exit_to_the_starting_session() {
    let scratch = scratch();
    let root = scratch.path();
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for name in ["sh", "sleep"] {
        symlink(tool(name), tools.join(name)).unwrap();
    }
    let outbox = root.join(".local/state/north/fleet/.outbox");
    fs::create_dir_all(&outbox).unwrap();
    fs::write(outbox.join("relay"), "fleet-host\n").unwrap();
    let release = root.join("release");
    let job = format!("until [ -f {} ]; do sleep 0.1; done; echo \"finished with ${{JOB_RUN_LOG:-no log}} set\"; exit 3", release.display());
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command
        .args(["job-run", "--path"])
        .arg(root.join("bin"))
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
    let begun = Instant::now();
    let letter: serde_json::Value = loop {
        let posted = fs::read_dir(&outbox).unwrap().flatten().find(|entry| entry.path().extension().is_some_and(|ext| ext == "json"));
        if let Some(entry) = posted {
            break serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "no letter posted");
        std::thread::sleep(Duration::from_millis(100));
    };
    let text = letter["text"].as_str().unwrap();
    assert_eq!(letter["to"].as_str(), Some("c-test"));
    assert!(text.contains("job-run: soak exited 3; output in "), "{text}");
    let job_log = text.rsplit(' ').next().unwrap();
    assert!(fs::read_to_string(job_log).unwrap().contains("finished with no log set"), "{job_log}");
    assert!(ssh_log(root).iter().any(|line| line.contains("job-run soak") && line.ends_with("--rc 3")), "{:?}", ssh_log(root));
}

#[test]
fn job_run_finds_the_threads_script_in_the_harness_bin_when_the_ledger_is_local_and_its_path_has_no_threads() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let (bin, empty) = (root.join("bin"), root.join("empty"));
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&empty).unwrap();
    fs::write(bin.join("threads"), "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/threads.log\"\n").unwrap();
    fs::set_permissions(bin.join("threads"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command
        .args(["job-run", "checks", "--"])
        .arg(tool("true"))
        .env("HOME", root)
        .env("NORTH_HOME", root)
        .env_remove("THREADS_DB")
        .env("PATH", &empty);
    let mut child = {
        let _gate = fork_gate();
        command.spawn().unwrap()
    };
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let log = fs::read_to_string(root.join("threads.log")).unwrap_or_default();
    assert!(log.lines().any(|line| line.starts_with("job-run checks") && line.contains("--finished") && line.ends_with("--rc 0")), "{log:?}");
}
