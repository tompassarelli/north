use std::time::Duration;

use super::super::wait;
use super::daemon;

fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
}

#[test]
fn a_fixture_daemon_ends_with_its_test_thread_though_no_destructor_runs() {
    let pid = std::thread::spawn(|| {
        let child = daemon("sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        assert!(wait::until(Duration::from_secs(5), || std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim() == "sleep"), "the daemon never started");
        std::mem::forget(child);
        pid
    })
    .join()
    .unwrap();
    assert!(wait::until(Duration::from_secs(5), || !running(pid)), "the daemon {pid} outlived the test thread that spawned it");
}
