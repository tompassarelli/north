use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn threads(root: &Path, verb: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command
        .args(["threads", verb])
        .env("HOME", root)
        .env("THREADS_DB", root.join("threads.db"))
        .env("NORTH_BIN", env!("CARGO_BIN_EXE_north"))
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
    let mut probe = threads(root, "run-ids").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let finished = north_core::platform::wait::until(Duration::from_secs(30), || probe.try_wait().unwrap().is_some());
    drop(stdin);
    if !finished {
        probe.kill().unwrap();
    }
    let probe = probe.wait_with_output().unwrap();
    let mut stderr = String::new();
    ingest.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    ingest.wait().unwrap();

    assert!(probe.status.success(), "{}", String::from_utf8_lossy(&probe.stderr));
    assert!(finished, "a writer was still waiting behind an ingest still reading its input");
}

