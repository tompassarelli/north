use super::*;

fn owned(commands: &[&str]) -> Vec<String> {
    commands.iter().map(|command| (*command).to_owned()).collect()
}

#[test]
fn a_no_build_head_north_check_joins_no_shared_build() {
    let before_push = "CARGO_TARGET_DIR=target ./bin/head-north --no-build --code crates/a/ docs features-index --check";
    assert!(!runs_head_north(before_push));
    assert_eq!(command(&owned(&[before_push])), None);
    assert!(phases(owned(&[before_push])).is_err());
}

#[test]
fn building_head_north_checks_share_one_build_beside_a_no_build_one() {
    let after = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/a/ docs features-index --check";
    let before_push = "CARGO_TARGET_DIR=target ./bin/head-north --no-build --code crates/b/ citation-check";
    let phases = phases(owned(&[after, before_push])).unwrap();
    assert_eq!(phases.build, "CARGO_TARGET_DIR=target ./bin/head-north --code crates/a/ --build");
    assert_eq!(phases.head_north, owned(&[after]));
    assert_eq!(phases.others, owned(&[before_push]));
}
