use super::*;

const NORTH_CLIPPY: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets ${SAFE_PUSH_LAND_PACKAGES:---workspace} --locked --offline -- -D warnings";
const OWNERS: &str = "bash harness/scripts/canary.sh";
const INDEX: &str = "./bin/head-north docs features-index --check";

fn commands() -> Vec<String> {
    vec![OWNERS.to_owned(), NORTH_CLIPPY.to_owned(), INDEX.to_owned()]
}

fn packages(names: &[&str]) -> Scope {
    Scope::Packages(names.iter().map(|name| (*name).to_owned()).collect())
}

#[test]
fn the_listed_clippy_check_moves_ahead_of_every_other_check_once() {
    let plan = plan(&commands(), |_| true).unwrap();
    assert_eq!(plan.clippy, NORTH_CLIPPY);
    assert_eq!(plan.rest, [OWNERS, INDEX]);
}

#[test]
fn no_clippy_check_or_only_an_inferred_one_skips_the_step() {
    assert_eq!(plan(&[OWNERS.to_owned(), INDEX.to_owned()], |_| true), None);
    assert_eq!(plan(&commands(), |command| !command.contains("clippy")), None);
    assert_eq!(plan(&[], |_| true), None);
}

#[test]
fn the_order_line_names_clippy_first_with_its_crates_then_every_other_check() {
    let plan = plan(&commands(), |_| true).unwrap();
    let line = order(&plan, Some(&packages(&["leaf", "north"])));
    assert!(line.starts_with(&format!("safe-push: land check order: 1. clippy on the affected crates (-p leaf -p north): {NORTH_CLIPPY}")), "{line}");
    assert!(line.ends_with(&format!("; then 2 land check(s): {OWNERS}; {INDEX}")), "{line}");
    assert!(order(&plan, Some(&Scope::Workspace)).contains("(--workspace)"));
    let alone = Plan { clippy: NORTH_CLIPPY.to_owned(), prebuild: None, rest: Vec::new() };
    assert_eq!(order(&alone, None), format!("safe-push: land check order: 1. clippy on the affected crates: {NORTH_CLIPPY}"));
}

const FEATURES: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-ops/src/ops/docs.rs,crates/north-ops/src/ops/features_index/ docs features-index --check";

#[test]
fn the_head_north_checks_after_clippy_share_one_build_beside_it() {
    let commands = [NORTH_CLIPPY.to_owned(), FEATURES.to_owned(), OWNERS.to_owned()];
    let planned = plan(&commands, |_| true).unwrap();
    let build = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-ops/src/ops/docs.rs,crates/north-ops/src/ops/features_index/ --build";
    assert_eq!(planned.first(), [NORTH_CLIPPY, build]);
    assert_eq!(planned.rest, [FEATURES, OWNERS]);
    assert!(order(&planned, None).contains(&format!(", beside it the north build the head-north checks share: {build}")));
    let alone = plan(&[NORTH_CLIPPY.to_owned(), OWNERS.to_owned()], |_| true).unwrap();
    assert_eq!(alone.first(), [NORTH_CLIPPY]);
}
