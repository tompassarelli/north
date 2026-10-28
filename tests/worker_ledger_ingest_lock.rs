use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const INGEST_WAIT: Duration = Duration::from_secs(5);
const RETRY: i32 = 75;

fn rig(root: &Path) {
    fs::create_dir_all(root.join("bin")).unwrap();
    let north = root.join("bin/north");
    fs::write(
        &north,
        format!("#!/bin/sh\ncase \"$1 $2\" in \"ledger \"*|\"threads ingest\") exec {} \"$@\";; esac\nprintf '[]\\n'\n", env!("CARGO_BIN_EXE_north")),
    )
    .unwrap();
    fs::set_permissions(&north, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"), root.join("bin/threads")).unwrap();
}

fn ledger(root: &Path) -> Output {
    Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/worker-ledger"))
        .args(["--since", "2026-10-08"])
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("THREADS_DB", root.join("threads.db"))
        .env("CLAUDE_CONFIG_DIR", root.join("claude"))
        .env("CODEX_CONFIG_DIR", root.join("codex"))
        .env("AGY_CONFIG_DIR", root.join("agy"))
        .env("LEDGER_REPOS", root.join("no-repos"))
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap()
}

#[test]
fn a_worker_ledger_ingest_waits_on_a_held_ingest_lock_then_refuses_for_retry_and_runs_once_it_is_free() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    rig(root);
    let held = File::create(root.join("ingest.lock")).unwrap();
    held.lock().unwrap();
    let started = Instant::now();
    let refused = ledger(root);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(RETRY), "{stderr}");
    assert!(stderr.contains("another ingest holds the ledger; retry"), "{stderr}");
    assert!(started.elapsed() >= INGEST_WAIT, "the ingest gave up after {:?} instead of waiting for the lock", started.elapsed());
    held.unlock().unwrap();
    let freed = ledger(root);
    assert!(freed.status.success(), "{}", String::from_utf8_lossy(&freed.stderr));
}
