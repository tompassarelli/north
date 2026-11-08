use super::lines::Record;
use super::outcome::{Outcome, parse};
use super::wrapped;

#[test]
fn nextest_status_lines_become_one_outcome_per_test_and_a_failure_listed_again_counts_once() {
    let text = "    Starting 4 tests across 2 binaries\n        PASS [   0.012s] north-base verdict_cache::tests::a\n        PASS [  12.500s] (  2/4) north::it land::b\n        SLOW [>60.000s] north::it land::slow\n        FAIL [   0.300s] north-base verdict_cache::tests::c\n     TRY 1 FAIL [   0.300s] north-base verdict_cache::tests::d\n  SIGABRT [   0.100s] north-base verdict_cache::tests::e\n------------\n        FAIL [   0.900s] north-base verdict_cache::tests::c\n";
    let found = parse(text);
    let named = |binary: &str, test: &str| found.iter().find(|outcome| outcome.binary == binary && outcome.test == test).cloned();
    assert_eq!(found.len(), 4);
    assert_eq!(named("north-base", "verdict_cache::tests::a"), Some(Outcome { binary: "north-base".into(), test: "verdict_cache::tests::a".into(), passed: true, seconds: 0.012 }));
    assert_eq!(named("north::it", "land::b").map(|outcome| outcome.seconds), Some(12.5));
    assert_eq!(named("north-base", "verdict_cache::tests::c").map(|outcome| (outcome.passed, outcome.seconds)), Some((false, 0.3)));
    assert_eq!(named("north-base", "verdict_cache::tests::e").map(|outcome| outcome.passed), Some(false));
}

#[test]
fn a_logged_record_parses_back_to_itself() {
    let unit = Record::Unit { probe: "abc".into(), unit: "north-base a::b".into(), set: Some("ff00".into()), passed: false, seconds: 1.5 };
    let differs = Record::SetDiffers { probe: "abc".into(), unit: "north-base a::b".into(), reused: "ff00".into(), real: "ee11".into(), path: "content harness/x".into() };
    assert_eq!(Record::parse(&unit.render()), Some(unit));
    assert_eq!(Record::parse(&differs.render()), Some(differs));
}

#[test]
fn the_wrapper_quotes_a_line_with_single_quotes_and_runs_the_plain_line_when_the_binary_lacks_the_verb() {
    let script = wrapped("north", "p1", true, "cargo nextest run -E 'binary_id(x)'");
    assert!(script.contains("land reuse available"));
    assert!(script.contains("--probe 'p1' --record -- 'cargo nextest run -E '\\''binary_id(x)'\\'''"));
    assert!(script.ends_with("\ncargo nextest run -E 'binary_id(x)'"));
}
