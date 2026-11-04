use super::*;
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

fn barrier(dir: &Path, name: &str, parties: usize) -> String {
    script(
        dir,
        name,
        &format!(
            "touch {dir}/{name}.ready\nfor _ in $(seq 1 200); do [ \"$(ls {dir}/*.ready | wc -l)\" -ge {parties} ] && {{ echo {name} met; exit 0; }}; sleep 0.05; done\necho {name} alone; exit 1\n",
            dir = dir.display()
        ),
    )
}

#[test]
fn selected_tests_run_side_by_side_up_to_the_job_limit() {
    let dir = tempfile::tempdir().unwrap();
    let tests: Vec<String> = (0..4).map(|i| barrier(dir.path(), &format!("t{i}.test.sh"), 4)).collect();
    let mut out = Vec::new();
    assert!(failed(run_all(&tests, NonZeroUsize::new(4).unwrap(), &mut out)).is_empty());
    let out = String::from_utf8(out).unwrap();
    for test in &tests {
        assert_eq!(out.lines().filter(|line| *line == format!("land check: {test}")).count(), 1);
    }
    assert_eq!(out.matches(" met").count(), 4, "{out}");
}

#[test]
fn a_red_test_fails_the_run_while_every_other_test_still_runs_and_reports_its_own_output() {
    let dir = tempfile::tempdir().unwrap();
    let red = script(dir.path(), "red.test.sh", "echo red says why\nexit 3\n");
    let green = script(dir.path(), "green.test.py", "print('green ran under python')\n");
    let later = script(dir.path(), "later.test.sh", "printf 'later ran'\n");
    let tests = vec![red.clone(), green.clone(), later.clone()];
    let mut out = Vec::new();
    assert_eq!(failed(run_all(&tests, NonZeroUsize::MIN, &mut out)), vec![red.clone()]);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains(&format!("land check: {red}\nred says why\nland check: {red} exited 3 after ")), "{out}");
    assert!(out.contains(&format!("land check: {green}\ngreen ran under python\nland check: {green} passed")), "{out}");
    assert!(out.contains(&format!("land check: {later}\nlater ran\nland check: {later} passed")), "{out}");
}

#[test]
fn a_built_test_executable_without_an_extension_runs_as_itself() {
    let dir = tempfile::tempdir().unwrap();
    let built = script(dir.path(), "units-0123abcd", "#!/bin/sh\necho ran as $0\nexit 4\n");
    fs::set_permissions(&built, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut out = Vec::new();
    assert_eq!(failed(run_all(std::slice::from_ref(&built), NonZeroUsize::MIN, &mut out)), vec![built.clone()]);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains(&format!("land check: {built}\nran as {built}\nland check: {built} exited 4 after ")), "{out}");
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

#[test]
fn a_shell_test_yields_one_row_per_pass_or_fail_line() {
    let log = "PASS hook names the file\nFAIL hook skips the lane\nPASS: third\nPASSING is not a case\n";
    assert_eq!(
        rows(&outcome("a.test.sh", 1, log)),
        vec![("hook names the file".into(), true), ("hook skips the lane".into(), false), ("third".into(), true)]
    );
}

#[test]
fn a_test_printing_neither_yields_one_whole_file_row_by_exit_status() {
    assert_eq!(rows(&outcome("quiet.test.sh", 0, "all good\n")), vec![("quiet.test.sh".into(), true)]);
    assert_eq!(rows(&outcome("quiet.test.sh", 2, "boom\n")), vec![("quiet.test.sh".into(), false)]);
}

#[test]
fn a_nonzero_exit_after_only_passing_lines_still_records_the_file_as_failed() {
    assert_eq!(
        rows(&outcome("crash.test.sh", 1, "PASS first\n")),
        vec![("first".into(), true), ("crash.test.sh".into(), false)]
    );
}

#[test]
fn cases_json_prints_rows_for_real_fixtures_in_argument_order() {
    let dir = tempfile::tempdir().unwrap();
    let shell = script(dir.path(), "a.test.sh", "echo 'PASS one'\necho 'FAIL two'\nexit 1\n");
    let libtest = script(dir.path(), "b.test.sh", "echo 'test m::x ... ok'\necho 'test m::y ... FAILED'\nexit 101\n");
    let quiet = script(dir.path(), "c.test.sh", "echo hello\n");
    let tests = vec![shell.clone(), libtest.clone(), quiet.clone()];
    let mut out = Vec::new();
    let outcomes = run_all(&tests, NonZeroUsize::new(3).unwrap(), &mut out);
    let rows: Vec<String> = tests
        .iter()
        .flat_map(|test| outcomes.iter().filter(move |outcome| outcome.test == *test))
        .flat_map(cases)
        .map(|case| case.row())
        .collect();
    let row = |test: &str, name: &str, outcome: &str| serde_json::json!({"test": test, "case": name, "outcome": outcome}).to_string();
    assert_eq!(
        rows,
        vec![
            row(&shell, "one", "pass"),
            row(&shell, "two", "fail"),
            row(&libtest, "m::x", "pass"),
            row(&libtest, "m::y", "fail"),
            row(&quiet, &quiet, "pass"),
        ]
    );
}
