use super::*;
use std::cell::Cell;

fn test(binary: &str, name: &str) -> Test {
    Test { binary: binary.to_owned(), name: name.to_owned() }
}

const LOG: &str = "        PASS [   0.177s] (  25/656) north::it land_route::a
        FAIL [   2.328s] (  26/656) north::it carry_matrix::b
     Summary [  11.681s] 43/656 tests run: 42 passed, 1 failed, 0 skipped
        FAIL [   2.328s] (  26/656) north::it carry_matrix::b
     SIGKILL [  30.001s] north-base platform::c
FAIL: a thawed land check outlived its timeout
";

#[test]
fn the_failed_tests_are_the_nextest_failure_lines_and_nothing_else() {
    assert_eq!(failed(LOG), [test("north::it", "carry_matrix::b"), test("north-base", "platform::c")].into());
}

#[test]
fn a_planted_miss_is_named_and_starts_one_refresh() {
    let selection = Selection::Tests { tests: [test("north::it", "carry_matrix::b")].into(), file_level: Vec::new(), added: Vec::new() };
    let refreshes = Cell::new(0);
    let code = judge(&selection, LOG, || {
        refreshes.set(refreshes.get() + 1);
        Ok(())
    });
    assert_eq!((code, refreshes.get()), (MISSED, 1));
    assert_eq!(agreement(&selection, &failed(LOG)), Agreement::Missed(vec![test("north-base", "platform::c")]));
}

#[test]
fn a_selection_that_ran_every_failed_test_agrees_and_one_that_ran_nothing_is_not_judged() {
    let ran = Selection::Tests { tests: failed(LOG), file_level: Vec::new(), added: Vec::new() };
    let refreshes = Cell::new(0);
    let refresh = || {
        refreshes.set(refreshes.get() + 1);
        Ok(())
    };
    assert_eq!(judge(&ran, LOG, refresh), 0);
    assert_eq!(judge(&Selection::Unmapped("Cargo.toml is a build input".to_owned()), LOG, refresh), 0);
    assert_eq!(refreshes.get(), 0);
    let over: BTreeSet<Test> = (0..=MOST_BEFORE_PUSH).map(|index| test("north-base", &index.to_string())).collect();
    assert!(matches!(agreement(&Selection::Tests { tests: over, file_level: Vec::new(), added: Vec::new() }, &failed(LOG)), Agreement::NotJudged(_)));
}
