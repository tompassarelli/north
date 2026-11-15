use std::collections::BTreeSet;

use super::*;
use super::super::lane_tree::manifest_dependencies;

const BUCK_CHECK: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-buck/src/,crates/north-build-system/src/,src/cli/ --no-build buck targets --check";
const TESTS: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=fail --workspace";
const JUDGE_TEST: &str = "private_kinds::tests::a_judged_change_names_its_kind";
const JUDGE_FILE: &str = "crates/north-land-scans/src/private_kinds_tests.rs";

fn buck_log(worktree: &str, stale: &[&str]) -> String {
    let mut log = format!("head-north: running the north this host built from tree {worktree:.12}\nnorth buck targets: north: build.rs is not run under Buck2 yet, so its targets compile the files it writes as committed\n");
    for file in stale {
        log.push_str(&format!("/home/tom/code/north/worktrees/{worktree}/{file} is not what north buck targets writes from the Cargo manifests; run it and commit the result\n"));
    }
    log
}

fn test_log(failing: &[&str]) -> String {
    let mut log = String::from("    Starting 1273 tests across 92 binaries\n");
    for test in failing {
        log.push_str(&format!("        FAIL [   0.120s] (  12/1273) north-land-scans {test}\n"));
    }
    log.push_str(&format!("     Summary [  41.002s] 1273 tests run: {} passed, {} failed, 0 skipped\n", 1273 - failing.len(), failing.len()));
    log
}

const MANIFESTS: [(&str, &str); 4] = [
    ("", "[package]\nname = \"north\"\n[dependencies]\nnorth-land-checks = { path = \"crates/north-land-checks\" }\n"),
    ("crates/north-land-scans/", "[dependencies]\nnorth-base = { path = \"../north-base\" }\nregex.workspace = true\n"),
    ("crates/north-land-checks/", "[dev-dependencies]\nnorth-base = { path = \"../north-base\" }\n"),
    ("crates/north-base/", "[dependencies]\n"),
];
const LANDING_TEST: &str = "land_checks::a_test_that_is_red_on_the_base_never_refuses_the_lane";

struct Tree;

impl Repository for Tree {
    fn exists(&self, inner: &str) -> bool {
        (inner.ends_with("/BUCK") && inner.starts_with("crates/")) || MANIFESTS.iter().any(|(package, _)| inner == format!("{package}Cargo.toml"))
    }

    fn test_files(&self, name: &str) -> Vec<String> {
        match name {
            JUDGE_TEST => vec![JUDGE_FILE.to_owned()],
            LANDING_TEST => vec!["tests/it/land_checks.rs".to_owned()],
            "fleet::tests::a_launch_keeps_its_account" => vec!["crates/north-fleet/src/lib.rs".to_owned()],
            _ => Vec::new(),
        }
    }

    fn path_dependencies(&self, package: &str) -> Option<Vec<String>> {
        MANIFESTS.iter().find(|(dir, _)| *dir == package).and_then(|(dir, manifest)| manifest_dependencies(dir, manifest))
    }
}

fn judged(command: &str, lane: &str, base: &str, changed: &[&str]) -> Attribution {
    declared(command, command == BUCK_CHECK, lane, base, changed)
}

fn declared(command: &str, reports_all: bool, lane: &str, base: &str, changed: &[&str]) -> Attribution {
    let changed: Vec<String> = changed.iter().map(|path| (*path).to_owned()).collect();
    let tests = command == TESTS;
    let base: BTreeSet<String> = signature(base, tests, &Tree);
    attribute(&LaneRun { command, log: lane, changed: &changed, tests, reports_all }, &base, &Tree)
}

#[test]
fn a_lane_is_refused_only_for_its_own_failure() {
    let base = buck_log("main-base", &["crates/north-docs/BUCK"]);
    let same = buck_log("lag-alarm-1540", &["crates/north-docs/BUCK"]);
    assert_eq!(judged(BUCK_CHECK, &same, &base, &["crates/north-land-watch/src/watch/lag.rs"]), Attribution::Base, "the stale BUCK main already had on 2026-11-15 refused lag-alarm-1540, which never touched it");

    let worse = buck_log("lag-alarm-1540", &["crates/north-docs/BUCK", "crates/north-capacity-host/BUCK"]);
    assert!(matches!(judged(BUCK_CHECK, &worse, &base, &["crates/north-capacity-host/Cargo.toml"]), Attribution::Lane(added) if added.len() == 1 && added[0].starts_with("crates/north-capacity-host/BUCK is not what")), "a lane that leaves a second BUCK stale adds a failure main lacks");

    assert!(matches!(declared(BUCK_CHECK, false, &same, &base, &["crates/north-land-watch/src/watch/lag.rs"]), Attribution::Unjudged(_)), "a check the base never declared reports every error may hide the lane's behind main's first");
    assert_eq!(judged(BUCK_CHECK, &same, &base, &["crates/north-docs/BUCK"]), Attribution::Touches(vec!["crates/north-docs/BUCK".to_owned()]), "a lane that edits the stale file itself owns its failure");
    assert_eq!(judged(BUCK_CHECK, &same, &base, &["crates/north-buck/src/lib.rs"]), Attribution::Touches(vec!["crates/north-buck/src/lib.rs".to_owned()]), "a lane that changes the check's own code owns its failure");

    let red_main = test_log(&[JUDGE_TEST]);
    assert_eq!(judged(TESTS, &red_main, &red_main, &["crates/north-fleet/src/lib.rs"]), Attribution::Base, "the disclosure-judge test broken on main since 598ce8b7b1 does not refuse a lane that never touched it");
    assert_eq!(judged(TESTS, &red_main, &red_main, &[JUDGE_FILE]), Attribution::Touches(vec![JUDGE_FILE.to_owned()]), "a lane that edits the failing test owns its failure");
    assert_eq!(judged(TESTS, &red_main, &red_main, &["crates/north-land-scans/src/private_kinds.rs"]), Attribution::Touches(vec!["crates/north-land-scans/src/private_kinds.rs".to_owned()]), "a lane that breaks the code a red test exercises fails that same test, so the test's package is the check's own");

    assert_eq!(judged(TESTS, &red_main, &red_main, &["crates/north-base/src/lib.rs"]), Attribution::Touches(vec!["crates/north-base/src/lib.rs".to_owned()]), "a red test exercises every package its own depends on by path, so a lane breaking one of them owns the failure");

    let red_landing = test_log(&[LANDING_TEST]);
    assert_eq!(judged(TESTS, &red_landing, &red_landing, &["crates/north-land-checks/src/land/slots/runner/judge.rs"]), Attribution::Touches(vec!["crates/north-land-checks/src/land/slots/runner/judge.rs".to_owned()]), "a red integration test of the root package exercises the crates the root depends on");
    assert_eq!(judged(TESTS, &red_landing, &red_landing, &["docs/rationale/landing-train.md"]), Attribution::Base, "a lane outside the packages a red test exercises leaves it main's");

    let unfound = test_log(&["generated::case_3"]);
    assert!(matches!(judged(TESTS, &unfound, &unfound, &["docs/rationale/landing-train.md"]), Attribution::Unjudged(_)), "a red test no file defines by name exercises code no one can tell, so it cannot clear a lane");

    let turned =test_log(&[JUDGE_TEST, "fleet::tests::a_launch_keeps_its_account"]);
    assert_eq!(judged(TESTS, &turned, &red_main, &["crates/north-fleet/src/lib.rs"]), Attribution::Lane(vec!["cargo fleet::tests::a_launch_keeps_its_account".to_owned()]), "a lane that turns a passing test red is refused");

    let stopped = format!("{red_main}warning: 1272/1273 tests were not run due to test failure (run with --no-fail-fast to run all tests, or run with --max-fail)\n");
    assert!(matches!(judged(TESTS, &stopped, &red_main, &["crates/north-fleet/src/lib.rs"]), Attribution::Unjudged(_)), "a run that stopped at main's failure never ran the lane's other tests");

    let compile = "error[E0425]: cannot find value `x` in this scope\n  --> crates/north-base/src/lib.rs:3:5\nerror: could not compile `north-base` (lib) due to 1 previous error\nwarning: build failed, waiting for other jobs to finish...\n";
    assert!(matches!(judged(TESTS, compile, compile, &["crates/north-fleet/src/lib.rs"]), Attribution::Unjudged(_)), "cargo stops at main's compile error, so a crate the lane breaks may never have compiled");

    let unsummarized = "error[E0425]: cannot find value `x` in this scope\n  --> crates/north-base/src/lib.rs:3:5\n";
    assert!(matches!(judged(TESTS, unsummarized, unsummarized, &["crates/north-fleet/src/lib.rs"]), Attribution::Unjudged(_)), "a test check that ran no test while main fails to compile judged nothing of the lane");

    let offline = "error: failed to download `serde v1.0.0`\n--offline was specified\n";
    assert!(matches!(judged(TESTS, offline, offline, &["crates/north-fleet/src/lib.rs"]), Attribution::Unjudged(_)), "a check that failed in its environment on both trees judged neither");
}
