use std::time::{Duration, Instant};

use super::daemon;

fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
}

// KEEP(bug): a fixture daemon whose test panicked under Cranelift, which runs no destructor, held the Cargo shard's output open until the job timed out; regression of north#673
#[test]
fn a_fixture_daemon_ends_with_its_test_thread_though_no_destructor_runs() {
    let pid = std::thread::spawn(|| {
        let child = daemon("sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        let started = Instant::now();
        while std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim() != "sleep" {
            assert!(started.elapsed() < Duration::from_secs(5), "the daemon never started");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::mem::forget(child);
        pid
    })
    .join()
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(pid) {
        assert!(Instant::now() < deadline, "the daemon {pid} outlived the test thread that spawned it");
        std::thread::sleep(Duration::from_millis(20));
    }
}
