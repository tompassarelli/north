use crate::harness::*;
use std::fs::{self, File};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn a_runtime_a_live_session_host_runs_from_survives_pruning_until_that_host_exits() {
    let host = Host::new();
    host.built(&host.target);
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    let old = "1111111111111111111111111111111111111111";
    let newer = ["4444444444444444444444444444444444444444", "5555555555555555555555555555555555555555", "6666666666666666666666666666666666666666", "7777777777777777777777777777777777777777", "8888888888888888888888888888888888888888"];
    let aged = |sha: &str, seconds: u64| File::open(host.runtime(sha)).unwrap().set_modified(UNIX_EPOCH + Duration::from_secs(seconds)).unwrap();
    host.built(old);
    fs::copy(env!("CARGO_BIN_EXE_north"), host.runtime(old).join("north")).unwrap();
    aged(old, 1_000);
    for (index, sha) in newer.iter().enumerate() {
        host.built(sha);
        aged(sha, 2_000 + index as u64);
    }
    aged(PREVIOUS, 3_000);
    let session = host.path("session");
    let mut session_host = Command::new(host.runtime(old).join("north"))
        .args(["session-host", "--name", "fixture", "--generation", "1", "--state-dir"])
        .arg(&session)
        .arg("--socket")
        .arg(session.join("host.sock"))
        .args(["--", "sleep", "600"])
        .spawn()
        .unwrap();
    while !session.join("host.sock").exists() {
        assert!(session_host.try_wait().unwrap().is_none(), "the fake session host exited before the prune");
        std::thread::sleep(Duration::from_millis(20));
    }

    let promoted = host.update(0, 0);
    assert!(promoted.status.success(), "{}", String::from_utf8_lossy(&promoted.stderr));
    assert!(!host.runtime(newer[0]).exists(), "the promotion pruned nothing, so it proves nothing");
    assert!(host.runtime(old).join("north").is_file(), "pruned the runtime a live session host runs from");

    assert!(Command::new("kill").arg(session_host.id().to_string()).status().unwrap().success());
    session_host.wait().unwrap();
    let source = host.path("source");
    git(&host.path("home"), &source, &["commit", "-q", "--allow-empty", "-m", "next landing"]);
    let next = git(&host.path("home"), &source, &["rev-parse", "HEAD"]);
    host.built(&next);
    let promoted = host.north_with(&["runtime", "update"], &[("TEST_TARGET", &next)]);
    assert!(promoted.status.success(), "{}", String::from_utf8_lossy(&promoted.stderr));
    assert!(!host.runtime(old).exists(), "kept the runtime after its last process exited");
}
