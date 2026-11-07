use super::*;
use crate::landing::land_test_selection::ChildEnv;
use std::fs;
use std::path::Path;

fn failed(outcomes: Vec<Outcome>) -> Vec<String> {
    outcomes.into_iter().filter(|outcome| !outcome.passed()).map(|outcome| outcome.test).collect()
}

fn script(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn a_red_test_fails_the_run_while_every_other_test_still_runs_and_reports_its_own_output() {
    let dir = tempfile::tempdir().unwrap();
    let red = script(dir.path(), "red.test.sh", "echo red says why\nexit 3\n");
    let green = script(dir.path(), "green.test.py", "print('green ran under python')\n");
    let later = script(dir.path(), "later.test.sh", "printf 'later ran'\n");
    let tests = vec![red.clone(), green.clone(), later.clone()];
    let mut out = Vec::new();
    assert_eq!(failed(run_all(&tests, NonZeroUsize::MIN, &ChildEnv::default(), &mut out)), vec![red.clone()]);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains(&format!("land check: {red}\nred says why\nland check: {red} exited 3 after ")), "{out}");
    assert!(out.contains(&format!("land check: {green}\ngreen ran under python\nland check: {green} passed")), "{out}");
    assert!(out.contains(&format!("land check: {later}\nlater ran\nland check: {later} passed")), "{out}");
}

fn outcome(test: &str, status: i32, output: &str) -> Outcome {
    Outcome { test: test.into(), status: Some(status), seconds: 0, output: output.as_bytes().to_vec() }
}

fn rows(outcome: &Outcome) -> Vec<(String, bool)> {
    cases(outcome).into_iter().map(|case| (case.name, case.passed)).collect()
}

#[test]
fn a_libtest_binary_yields_one_row_per_case_and_skips_ignored_ones() {
    let log = "running 4 tests\ntest a::ok_one ... ok\ntest a::bad ... FAILED\ntest a::skipped ... ignored\ntest a::slow ... ok <0.5s>\n\nfailures:\n---- a::bad stdout ----\n";
    assert_eq!(
        rows(&outcome("units-1", 101, log)),
        vec![("a::ok_one".into(), true), ("a::bad".into(), false), ("a::slow".into(), true)]
    );
}
