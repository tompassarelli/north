use std::process::Command;

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
