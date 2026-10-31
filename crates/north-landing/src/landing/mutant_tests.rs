use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::record;
use crate::landing::mutant::{Env, Lease, Stop};
use crate::landing::mutant_verify::verify;

const TEST: &str = "guards product";
const RUNNER: &str = r#"printf 'run\n' >>"$1"
for tree in $MUTANT_TREES; do
  printf '%s\n' "$MUTANT_TESTS" | while IFS= read -r name; do
    if grep -q good "$tree/product.txt"; then verdict=pass; else verdict=fail; fi
    printf '%s\t%s\n' "$verdict" "$name" >>"$tree/.mutant-results"
  done
done
"#;

struct Fixture {
    _root: tempfile::TempDir,
    lane: PathBuf,
    counter: PathBuf,
    env: Env,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(fixture: &Fixture, file: &str, text: &str, message: &str) -> String {
    fs::write(fixture.lane.join(file), text).unwrap();
    git(&fixture.lane, &["add", "-A"]);
    git(&fixture.lane, &["commit", "-qm", message]);
    git(&fixture.lane, &["rev-parse", "HEAD"])
}

fn fixture(runner: bool) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let lane = root.path().join("lane");
    git(root.path(), &["init", "-q", "--bare", "fixture.git"]);
    git(root.path(), &["init", "-q", "-b", "main", "lane"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.invalid"), ("commit.gpgsign", "false"), ("core.hooksPath", "/dev/null")] {
        git(&lane, &["config", key, value]);
    }
    git(&lane, &["remote", "add", "origin", &root.path().join("fixture.git").to_string_lossy()]);
    let counter = root.path().join("counter");
    fs::write(lane.join("runner.sh"), RUNNER).unwrap();
    let config = if runner { format!("[mutant]\n\trun = sh ./runner.sh {}\n", counter.display()) } else { String::new() };
    fs::write(lane.join(".safe-push"), config).unwrap();
    fs::write(lane.join("unrelated.txt"), "one\n").unwrap();
    let env = Env { state_home: root.path().join("state"), cache_home: root.path().join("cache"), lease: Lease::Inherited };
    let fixture = Fixture { _root: root, lane, counter, env };
    commit(&fixture, "product.txt", "good\n", "base");
    fixture
}

fn mutate(fixture: &Fixture, id: &str, file: &str, text: &str) {
    fs::write(fixture.lane.join(file), text).unwrap();
    assert!(record(&fixture.lane, id, &fixture.env).is_ok());
    git(&fixture.lane, &["checkout", "--", file]);
}

fn claim(fixture: &Fixture, id: &str, product: &str) -> String {
    commit(fixture, "product.txt", product, &format!("feature\n\nTest-kind: {TEST} 3 kills {id}"))
}

fn check(fixture: &Fixture, base: &str, head: &str) -> (bool, String) {
    let mut out = Vec::new();
    let outcome = verify(&fixture.lane, base, head, &fixture.env, &mut out);
    assert!(!matches!(outcome, Err(Stop::Usage(_) | Stop::Fail(_))), "{}", String::from_utf8_lossy(&out));
    (outcome.is_ok(), String::from_utf8_lossy(&out).into_owned())
}

fn runs(fixture: &Fixture) -> usize {
    fs::read_to_string(&fixture.counter).map(|text| text.lines().count()).unwrap_or(0)
}

fn markers(fixture: &Fixture) -> usize {
    fs::read_dir(fixture.env.state_home.join("agents/mutants/fixture/kills")).map(|entries| entries.count()).unwrap_or(0)
}

fn run_dirs_left(fixture: &Fixture) -> usize {
    fs::read_dir(fixture.env.cache_home.join("north/mutant/fixture")).map(|entries| entries.count()).unwrap_or(0)
}

#[test]
fn a_test_that_passes_with_its_mutant_applied_is_refused_without_a_marker() {
    let fixture = fixture(true);
    let base = git(&fixture.lane, &["rev-parse", "HEAD"]);
    let head = claim(&fixture, "harmless", "good and new\n");
    mutate(&fixture, "harmless", "unrelated.txt", "two\n");
    let (passed, out) = check(&fixture, &base, &head);
    assert!(!passed);
    assert!(out.contains(&format!("safe-push: ✗ Test-kind {TEST} does not kill mutant harmless: it passes (or did not run) with the mutant applied; NOT pushed.")), "{out}");
    assert_eq!((runs(&fixture), markers(&fixture), run_dirs_left(&fixture)), (1, 0, 0));
}

#[test]
fn a_test_that_fails_on_the_change_itself_is_refused() {
    let fixture = fixture(true);
    let base = git(&fixture.lane, &["rev-parse", "HEAD"]);
    let head = claim(&fixture, "flip", "broken\n");
    mutate(&fixture, "flip", "product.txt", "worse\n");
    let (passed, out) = check(&fixture, &base, &head);
    assert!(!passed);
    assert!(out.contains(&format!("safe-push: ✗ Test-kind {TEST} fails or did not run on the change itself, so it cannot vouch for mutant flip; NOT pushed.")), "{out}");
    assert_eq!(markers(&fixture), 0);
}

#[test]
fn a_verified_kill_is_cached_across_a_rebase_and_rerun_when_the_lane_changes() {
    let fixture = fixture(true);
    let base = git(&fixture.lane, &["rev-parse", "HEAD"]);
    let head = claim(&fixture, "flip", "good and new\n");
    mutate(&fixture, "flip", "product.txt", "bad\n");
    let (passed, out) = check(&fixture, &base, &head);
    assert!(passed && out.contains(&format!("mutant: ✓ {TEST} kills flip (")), "{out}");
    assert_eq!((runs(&fixture), markers(&fixture), run_dirs_left(&fixture)), (1, 1, 0));

    git(&fixture.lane, &["checkout", "-q", "-b", "next", &base]);
    let moved = commit(&fixture, "unrelated.txt", "moved\n", "unrelated main change");
    git(&fixture.lane, &["checkout", "-q", "main"]);
    git(&fixture.lane, &["rebase", "-q", &moved]);
    let rebased = git(&fixture.lane, &["rev-parse", "HEAD"]);
    assert_ne!(rebased, head);
    let (passed, out) = check(&fixture, &moved, &rebased);
    assert!(passed && out.contains(&format!("mutant: ✓ {TEST} kills flip (cached)")), "{out}");
    assert_eq!(runs(&fixture), 1);

    let extended = commit(&fixture, "feature.txt", "extra\n", "extra edit");
    let (passed, out) = check(&fixture, &moved, &extended);
    assert!(passed && !out.contains("(cached)"), "{out}");
    assert_eq!((runs(&fixture), markers(&fixture)), (2, 2));
}

#[test]
fn a_missing_or_empty_patch_is_refused_and_no_runner_only_checks_applicability() {
    let fixture = fixture(false);
    let base = git(&fixture.lane, &["rev-parse", "HEAD"]);
    let head = claim(&fixture, "absent", "good and new\n");
    let refusal = |id: &str| format!("safe-push: ✗ Test-kind {TEST} claims mutant {id}, which has no recorded non-empty patch; record it with mutant record; NOT pushed.");
    let (passed, out) = check(&fixture, &base, &head);
    assert!(!passed && out.contains(&refusal("absent")), "{out}");

    fs::create_dir_all(fixture.env.state_home.join("agents/mutants/fixture")).unwrap();
    fs::write(fixture.env.patch("fixture", "absent"), b"").unwrap();
    let (passed, out) = check(&fixture, &base, &head);
    assert!(!passed && out.contains(&refusal("absent")), "{out}");

    mutate(&fixture, "absent", "product.txt", "bad\n");
    assert_eq!(check(&fixture, &base, &head), (true, String::new()));
    assert_eq!(runs(&fixture), 0);

    let stale = commit(&fixture, "product.txt", "elsewhere\n", "rewrite\n\nTest-kind: other 2 kills absent");
    fs::write(fixture.env.patch("fixture", "absent"), b"diff --git a/product.txt b/product.txt\n--- a/product.txt\n+++ b/product.txt\n@@ -1 +1 @@\n-nowhere\n+bad\n").unwrap();
    let (passed, out) = check(&fixture, &base, &stale);
    assert!(!passed && out.contains(&refusal("absent")), "{out}");
}
