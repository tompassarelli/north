use std::process::Command;
use std::time::Duration;

use north_base::platform::wait;

use super::sampler::terminate;
use super::sampling::Term;

fn start_of(pid: u32) -> String {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat[stat.rfind(')').unwrap() + 2..].split(' ').nth(19).unwrap().to_owned()
}

#[test]
fn termination_signals_only_a_process_whose_start_time_still_matches() {
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid = i64::from(child.id());
    terminate(&[Term { pid, start: "0".into() }]);
    assert!(!wait::until(Duration::from_millis(200), || child.try_wait().unwrap().is_some()), "a recycled pid with another start time is left alone");
    terminate(&[Term { pid, start: start_of(child.id()) }]);
    assert!(wait::until(Duration::from_secs(5), || child.try_wait().unwrap().is_some()), "the matching process was not terminated");
}
