use super::try_run_command_traced_within;
use std::process::Command;
use std::time::Duration;

#[test]
fn a_command_that_outlives_its_limit_reports_what_each_of_its_processes_waits_on() {
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 2 & wait"]);
    let stalled = try_run_command_traced_within(command, None, Duration::from_millis(300)).unwrap().unwrap_err();
    let lines: Vec<&str> = stalled.split("; ").collect();
    assert_eq!(lines.len(), 2, "{stalled}");
    assert!(lines[0].starts_with("pid ") && lines[0].ends_with(": sh -c sleep 2 & wait"), "{stalled}");
    assert!(lines[1].starts_with("> pid ") && lines[1].contains(" state S ") && lines[1].ends_with(": sleep 2"), "{stalled}");
}

#[test]
fn a_command_that_finishes_in_time_returns_its_output() {
    let mut command = Command::new("sh");
    command.args(["-c", "cat; echo done"]);
    let output = try_run_command_traced_within(command, Some(b"in "), Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!(output.stdout, b"in done\n");
}
