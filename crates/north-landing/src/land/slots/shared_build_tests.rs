use super::*;

const FEATURES: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-ops/src/ops/docs.rs,crates/north-ops/src/ops/features_index/ docs features-index --check";
const CITATION: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/,crates/north-ops/src/ops/docs.rs citation-check --root $PWD";
const LAND_TESTS: &str = "CARGO_TARGET_DIR=target ./bin/head-north land-tests --select changed --only portable";
const TESTS: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo test --locked --offline";

fn owned(commands: &[&str]) -> Vec<String> {
    commands.iter().map(|command| (*command).to_owned()).collect()
}

#[test]
fn the_shared_build_covers_the_union_of_the_head_north_code_lists_with_their_environment() {
    assert_eq!(command(&owned(&[TESTS, FEATURES, CITATION])).as_deref(), Some("CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-ops/src/ops/docs.rs,crates/north-ops/src/ops/features_index/,src/cli/ --build"));
    assert_eq!(command(&owned(&[FEATURES, LAND_TESTS])).as_deref(), Some("CARGO_TARGET_DIR=target ./bin/head-north --build"), "a head-north check without a code list judges every Rust input");
    assert_eq!(command(&owned(&[TESTS])), None);
}

#[test]
fn the_head_north_checks_wait_for_the_build_while_every_other_check_runs_beside_both() {
    let split = phases(owned(&[FEATURES, TESTS, CITATION])).unwrap();
    assert_eq!((split.others, split.build.as_str()), (owned(&[TESTS]), "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-ops/src/ops/docs.rs,crates/north-ops/src/ops/features_index/,src/cli/ --build"));
    assert_eq!(split.head_north, owned(&[FEATURES, CITATION]));
    assert_eq!(phases(owned(&[TESTS])), Err(owned(&[TESTS])));
}
