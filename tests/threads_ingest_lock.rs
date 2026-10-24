use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn threads(root: &Path, verb: &str) -> Command {
    let mut command = Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"));
    command
        .arg(verb)
        .env("HOME", root)
        .env("THREADS_DB", root.join("threads.db"))
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    command
}

#[test]
fn a_slow_ingest_stream_does_not_hold_the_ledger_write_lock() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    assert!(threads(root, "run-ids").status().unwrap().success());

    let mut ingest = threads(root, "ingest").stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let stdin = ingest.stdin.take().unwrap();
    std::thread::sleep(Duration::from_secs(2));

    let started = Instant::now();
    let probe = threads(root, "run-ids").output().unwrap();
    let waited = started.elapsed();
    drop(stdin);
    let mut stderr = String::new();
    ingest.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    ingest.wait().unwrap();

    assert!(probe.status.success(), "{}", String::from_utf8_lossy(&probe.stderr));
    assert!(waited < Duration::from_secs(5), "a writer waited {waited:?} behind an ingest still reading its input");
}

#[test]
fn cost_allocation_is_indexed_by_agent_so_its_write_lock_stays_short() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    assert!(threads(root, "run-ids").status().unwrap().success());
    let db = root.join("threads.db");
    let setup = "create table usage(ts,provider,account,kind,name,used_pct,resets_at,window_min)";
    assert!(Command::new("sqlite3").arg(&db).arg(setup).status().unwrap().success());
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/lib/fixtures");
    let ingest = threads(root, "ingest")
        .env("AGENTS_OPERATOR_DIR", fixtures)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(ingest.status.success(), "{}", String::from_utf8_lossy(&ingest.stderr));
    let indexes = Command::new("sqlite3").arg(&db).arg(".indexes run_costs").output().unwrap();
    assert!(String::from_utf8_lossy(&indexes.stdout).contains("run_costs_agent"));
}
