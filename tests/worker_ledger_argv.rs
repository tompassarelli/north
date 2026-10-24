use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const RUNS: u32 = 40_000;

fn ledger(root: &Path) -> Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/worker-ledger");
    Command::new(script)
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
fn recorded_run_history_reaches_the_join_steps_without_argv() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    let north = root.join("bin/north");
    fs::write(&north, format!("#!/bin/sh\n[ \"$1\" = ledger ] && exec {} \"$@\"\nprintf '[]\\n'\n", env!("CARGO_BIN_EXE_north"))).unwrap();
    fs::set_permissions(&north, fs::Permissions::from_mode(0o755)).unwrap();
    ledger(root);
    let insert = format!(
        "with recursive n(i) as (select 1 union all select i+1 from n where i<{RUNS}) \
         insert or ignore into runs(agent,tier,category,ended,minutes,outcome) \
         select 'bulk-' || printf('%040d', i), 'x', 'x', '2026-10-08T00:00:00Z', 0, 'done' from n"
    );
    let loaded = Command::new("sqlite3").arg(root.join("threads.db")).arg(insert).output().unwrap();
    assert!(loaded.status.success(), "{}", String::from_utf8_lossy(&loaded.stderr));
    let run = ledger(root);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!stderr.contains("Argument list too long"), "{stderr}");
    assert!(run.status.success(), "{stderr}");
}
