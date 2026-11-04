use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::check;
use crate::landing::mutant::{Env, Lease, Stop};

const LIB: &str = "crates/demo/src/lib.rs";
const TESTS: &str = "crates/demo/src/lib_tests.rs";
const RUNNER: &str = r#"printf 'run\n' >>"$1"
if grep -q uses_new_api "$PWD/crates/demo/src/lib_tests.rs" && ! grep -q uses_new_api "$PWD/crates/demo/src/lib.rs"; then
  printf 'error: could not compile `demo` (lib test) due to 1 previous error\n'
  exit 101
fi
for tree in $MUTANT_TREES; do
  printf '%s\n' "$MUTANT_TESTS" | while IFS= read -r name; do
    if grep -qx "$name" "$tree/crates/demo/src/lib.rs"; then verdict=pass; else verdict=fail; fi
    printf '%s\t%s\n' "$verdict" "$name" >>"$tree/.mutant-results"
  done
done
"#;

struct Fixture {
    _root: tempfile::TempDir,
    lane: PathBuf,
    counter: PathBuf,
    env: Env,
    base: String,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn tests(names: &[(&str, &str)]) -> String {
    names.iter().map(|(name, body)| format!("#[test]\nfn {name}() {{\n    {body}\n}}\n\n")).collect()
}

fn commit(lane: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let path = lane.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    git(lane, &["add", "-A"]);
    git(lane, &["commit", "-qm", message]);
    git(lane, &["rev-parse", "HEAD"])
}

fn fixture(lib: &str) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let lane = root.path().join("lane");
    git(root.path(), &["init", "-q", "-b", "main", "lane"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.invalid"), ("commit.gpgsign", "false"), ("core.hooksPath", "/dev/null")] {
        git(&lane, &["config", key, value]);
    }
    let counter = root.path().join("counter");
    let config = format!("[mutant]\n\trun = sh ./runner.sh {}\n", counter.display());
    let existing = tests(&[("existing", "assert!(true);")]);
    let base = commit(&lane, &[("runner.sh", RUNNER), (".safe-push", &config), (LIB, lib), (TESTS, &existing)], "base");
    let env = Env { state_home: root.path().join("state"), cache_home: root.path().join("cache"), lease: Lease::Inherited };
    Fixture { _root: root, lane, counter, env, base }
}

fn gate(fixture: &Fixture, head: &str) -> (bool, String) {
    let mut out = Vec::new();
    let outcome = check(&fixture.lane, &fixture.base, head, &fixture.env, &mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(!matches!(outcome, Err(Stop::Usage(_) | Stop::Fail(_))), "{text}");
    (outcome.is_ok(), text)
}

fn runs(fixture: &Fixture) -> usize {
    fs::read_to_string(&fixture.counter).map(|text| text.lines().count()).unwrap_or(0)
}

#[test]
fn a_fix_whose_new_test_fails_on_base_passes() {
    let fixture = fixture("old\n");
    let added = format!(
        "{}proptest! {{\n    fn prop_round(x in 0u8..) {{\n        assert!(x >= 0);\n    }}\n}}\n",
        tests(&[("existing", "assert!(true);"), ("guards_bug", "assert!(fixed());")])
    );
    let head = commit(&fixture.lane, &[(LIB, "guards_bug\n"), (TESTS, &added)], "fix the bug");
    let (passed, out) = gate(&fixture, &head);
    assert!(passed, "{out}");
    assert!(out.contains("fails-first: on base guards_bug fails"), "{out}");
    assert!(out.contains("fails-first: on base prop_round fails"), "{out}");
    assert!(out.contains("fails-first: ✓ 2 of 2 named test(s) fail on base: guards_bug, prop_round"), "{out}");
    assert!(!out.contains("existing"), "{out}");
    assert_eq!(runs(&fixture), 1);
}

#[test]
fn a_lane_whose_changed_tests_all_pass_on_base_is_refused_with_their_names() {
    let fixture = fixture("old\nguards_bug\n");
    let added = tests(&[("existing", "assert!(true);"), ("guards_bug", "assert!(fixed());")]);
    let head = commit(&fixture.lane, &[(LIB, "new\nguards_bug\n"), (TESTS, &added)], "fix the bug");
    let (passed, out) = gate(&fixture, &head);
    assert!(!passed, "{out}");
    assert!(out.contains("every added or changed test passes on base, so none guards the change: guards_bug;"), "{out}");
}

#[test]
fn a_behavior_unchanged_lane_whose_changed_tests_fail_on_base_is_refused() {
    let fixture = fixture("old\n");
    let changed = tests(&[("existing", "assert_eq!(1, 1);")]);
    let head = commit(&fixture.lane, &[(LIB, "existing\n"), (TESTS, &changed)], "refactor\n\nBehavior: unchanged");
    let (passed, out) = gate(&fixture, &head);
    assert!(!passed, "{out}");
    assert!(out.contains("marked Behavior: unchanged, but these tests fail on base, so the change alters behavior: existing;"), "{out}");
}

#[test]
fn a_behavior_unchanged_lane_whose_changed_tests_pass_on_base_passes() {
    let fixture = fixture("existing\n");
    let changed = tests(&[("existing", "assert_eq!(1, 1);")]);
    let head = commit(&fixture.lane, &[(LIB, "existing\nrefactored\n"), (TESTS, &changed)], "refactor\n\nBehavior: unchanged");
    let (passed, out) = gate(&fixture, &head);
    assert!(passed, "{out}");
    assert!(out.contains("fails-first: on base existing passes"), "{out}");
}

#[test]
fn a_lane_changing_only_code_or_only_tests_is_not_checked() {
    let fixture = fixture("old\n");
    let code = commit(&fixture.lane, &[(LIB, "new\n")], "code only");
    let (passed, out) = gate(&fixture, &code);
    assert!(passed && out.contains("fails-first: not applicable"), "{out}");
    git(&fixture.lane, &["reset", "-q", "--hard", &fixture.base]);
    let only_tests = commit(&fixture.lane, &[(TESTS, &tests(&[("existing", "assert_eq!(2, 2);")]))], "tests only");
    let (passed, out) = gate(&fixture, &only_tests);
    assert!(passed && out.contains("fails-first: not applicable"), "{out}");
    assert_eq!(runs(&fixture), 0);
}

#[test]
fn a_lane_whose_test_file_cannot_be_placed_on_base_is_unverified_not_unavailable() {
    let fixture = fixture("old\n");
    let blocked = commit(&fixture.lane, &[("crates/demo/src/checks", "a file\n")], "a file where the lane puts a directory");
    let fixture = Fixture { base: blocked, ..fixture };
    git(&fixture.lane, &["rm", "-q", "crates/demo/src/checks"]);
    let case = tests(&[("guards_bug", "assert!(fixed());")]);
    let head = commit(&fixture.lane, &[(LIB, "guards_bug\n"), ("crates/demo/src/checks/tests/case.rs", &case)], "fix the bug");
    let (passed, out) = gate(&fixture, &head);
    assert!(passed, "{out}");
    assert!(out.contains("fails-first: unverified (the lane's test file crates/demo/src/checks/tests/case.rs cannot be placed on base:"), "{out}");
    assert_eq!(runs(&fixture), 0);
}

#[test]
fn a_fix_whose_new_test_does_not_compile_on_base_counts_as_failing_first() {
    let fixture = fixture("old\n");
    let added = tests(&[("existing", "assert!(true);"), ("new_api", "assert!(uses_new_api());")]);
    let head = commit(&fixture.lane, &[(LIB, "uses_new_api\nnew_api\n"), (TESTS, &added)], "add the api");
    let (passed, out) = gate(&fixture, &head);
    assert!(passed, "{out}");
    assert!(out.contains("fails-first: on base new_api does not compile"), "{out}");
    assert!(out.contains("fails-first: ✓ 1 of 1 named test(s) fail on base: new_api"), "{out}");
}

#[test]
fn a_behavior_unchanged_lane_whose_changed_tests_do_not_compile_on_base_is_unverified() {
    let fixture = fixture("existing\n");
    let changed = tests(&[("existing", "assert!(uses_new_api());")]);
    let head = commit(&fixture.lane, &[(LIB, "existing\nuses_new_api\n"), (TESTS, &changed)], "rename the api\n\nBehavior: unchanged");
    let (passed, out) = gate(&fixture, &head);
    assert!(passed, "{out}");
    assert!(out.contains("fails-first: unverified (the lane is marked Behavior: unchanged, but its changed tests do not compile on base"), "{out}");
}

#[test]
fn a_missing_base_is_an_infrastructure_fault() {
    let fixture = fixture("old\n");
    let added = tests(&[("existing", "assert!(true);"), ("guards_bug", "assert!(fixed());")]);
    let head = commit(&fixture.lane, &[(LIB, "guards_bug\n"), (TESTS, &added)], "fix the bug");
    let outcome = check(&fixture.lane, "0123456789abcdef0123456789abcdef01234567", &head, &fixture.env, &mut Vec::new());
    assert!(matches!(outcome, Err(Stop::Fail(_))));
}
