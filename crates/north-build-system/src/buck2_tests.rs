use super::Buck2;
use crate::cargo::Cargo;
use crate::BuildSystem;

/// The lint line, the Rust test line and one scan line of north's `.safe-push` on 2026-11-12.
const CLIPPY: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets ${SAFE_PUSH_LAND_CHANGED:---workspace} --locked --offline -- -D warnings";
const NEXTEST: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=fail ${SAFE_PUSH_LAND_PACKAGES:---workspace} --lib --bins ${SAFE_PUSH_LAND_IT:---test it -E kind(lib)|kind(bin)|kind(test)}";
const SCAN: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-buck/src/,crates/north-build-system/src/,src/cli/ --no-build buck targets --check";

#[test]
fn a_buck2_host_lints_with_its_own_gate_and_runs_its_own_tests_only_behind_the_test_switch() {
    let (builds, tests_too) = (Buck2 { tests: false }, Buck2 { tests: true });

    assert_eq!(builds.land_line(CLIPPY), "./bin/land-shell --dev -- buck2 bxl --materializations=none //lint/clippy.bxl:clippy -- --targets //...");
    assert_eq!(builds.land_line(NEXTEST), NEXTEST, "the test run stays Cargo's until the host sets buck2_tests");
    assert_eq!(tests_too.land_line(NEXTEST), "./bin/land-shell --dev -- buck2 test --unstable-allow-compatible-tests-on-re //...");
    assert_eq!((tests_too.land_line(SCAN), Cargo.land_line(CLIPPY)), (SCAN.to_owned(), CLIPPY.to_owned()), "a line that runs no lint or test, and every line on a Cargo host, runs as declared");
    assert!(crate::lints(CLIPPY) && crate::lints(&builds.land_line(CLIPPY)) && !crate::lints(NEXTEST), "the lint line of either side is told from a test line");
}
