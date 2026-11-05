use std::process::Command;
use std::time::{Duration, Instant};

use super::sampler::terminate;
use super::sampling::Term;

fn start_of(pid: u32) -> String {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat[stat.rfind(')').unwrap() + 2..].split(' ').nth(19).unwrap().to_owned()
}

// KEEP(mutant): SIGTERM only reaches a process whose start time still matches, never a recycled pid
#[test]
fn termination_signals_only_a_process_whose_start_time_still_matches() {
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid = i64::from(child.id());
    terminate(&[Term { pid, start: "0".into() }]);
    std::thread::sleep(Duration::from_millis(200));
    assert!(child.try_wait().unwrap().is_none(), "a recycled pid with another start time is left alone");
    terminate(&[Term { pid, start: start_of(child.id()) }]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the matching process was not terminated");
        std::thread::sleep(Duration::from_millis(20));
    }
}
