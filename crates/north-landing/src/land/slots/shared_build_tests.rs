use super::*;

fn owned(commands: &[&str]) -> Vec<String> {
    commands.iter().map(|command| (*command).to_owned()).collect()
}

#[test]
fn building_head_north_checks_share_one_build_beside_a_no_build_one() {
    let after = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/a/ docs features-index --check";
    let before_push = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/b/ --no-build citation-check";
    let phases = phases(owned(&[after, before_push])).unwrap();
    assert_eq!(phases.build, "CARGO_TARGET_DIR=target ./bin/head-north --code crates/a/ --build");
    assert_eq!(phases.head_north, owned(&[after]));
    assert_eq!(phases.others, owned(&[before_push]));
}
