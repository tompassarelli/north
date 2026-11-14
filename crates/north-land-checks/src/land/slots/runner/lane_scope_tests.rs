use std::process::Command;

use north_base::foundation::exit::NO_VERDICT_STATUS;

use super::{failed, wrapped};

#[test]
fn a_lane_is_named_only_by_the_wrappers_own_line_north_1039() {
    let lanes = ["a=1..2".to_owned(), "b=3..4".to_owned()];
    let command = "echo 'quoted log: land lane failed: a'; printf 'partial land lane failed: a'; test \"$SAFE_PUSH_LAND_BASE\" != 3";
    let output = Command::new("bash").args(["-c", &wrapped(command, &lanes)]).output().unwrap();
    assert!(!output.status.success());
    let log = String::from_utf8(output.stdout).unwrap();
    assert_eq!(failed(&log), ["b"], "{log}");
}

#[test]
fn a_run_that_could_not_run_blames_no_lane_north_1423() {
    let lanes = ["a=1..2".to_owned(), "b=3..4".to_owned()];
    let command = "echo \"fail-to-pass: could not run, not the lane's fault: the test runner reported no results\"; exit 75";
    let output = Command::new("bash").args(["-c", &wrapped(command, &lanes)]).output().unwrap();
    let log = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(NO_VERDICT_STATUS), "{log}");
    assert!(log.contains("could not run") && log.contains("no lane is blamed"), "{log}");
    assert!(failed(&log).is_empty(), "{log}");
}
