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

#[test]
fn a_backfilled_run_takes_its_reread_role_tier_and_item() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let row = |role: &str, tier: &str, backfill: bool| {
        format!(
            r#"{{"id":"sc-459","role":"{role}","tier":"{tier}","backfill":{backfill},"brief":"Item: smashcraft#459 back-air\nCategory: feature","status":"finished","outcome":"done","started":"2026-10-26T11:00:00Z","ended":"2026-10-26T11:05:00Z","actual_min":5}}"#
        )
    };
    for line in [row("proxy", "claude-opus-5-5", false), row("worker", "medium", true)] {
        let mut ingest = threads(root, "ingest").stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        std::io::Write::write_all(&mut ingest.stdin.take().unwrap(), format!("{line}\n").as_bytes()).unwrap();
        let output = ingest.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
    let query = "select role, tier, item, category from runs where agent = 'sc-459'";
    let run = Command::new("sqlite3").arg(root.join("threads.db")).arg(query).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "worker|medium|smashcraft#459|feature");
}
