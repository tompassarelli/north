use std::process::Command;

use super::*;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(root: &Path, path: &str, text: &str) -> String {
    fs::write(root.join(path), text).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", path]);
    git(root, &["rev-parse", "HEAD"])
}

fn at(root: &Path, revision: &str) {
    git(root, &["reset", "--hard", revision]);
}

fn gates(root: &Path, base: &str, head: &str, state_home: &Path, code_health: &Path) -> Vec<&'static str> {
    failed(&Lane { worktree: root, base, head, state_home, code_health, registry: &crate::dep_gate::PublicRegistries, advisory_db: &state_home.join("advisory-db") }).map(|(gates, _)| gates).unwrap_or_default()
}

fn repository() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (root, state_home) = (dir.path().join("repo"), dir.path().join("state"));
    fs::create_dir_all(root.join(".github/workflows")).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&root, &["config", key, value]);
    }
    let stub = dir.path().join("code-health");
    north_base::foundation::shim::write_script(&stub, "exit 0\n");
    fs::write(root.join("structure.toml"), "").unwrap();
    (dir, root, state_home, stub)
}

#[test]
fn a_lane_code_health_refuses_after_landing_is_refused_before_it_and_an_after_line_needs_a_check_or_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let (root, state_home) = (dir.path().join("repo"), dir.path().join("state"));
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&root, &["config", key, value]);
    }
    let stub = dir.path().join("code-health");
    north_base::foundation::shim::write_script(&stub, "grep -q complex triage.ts && exit 1\nexit 0\n");
    fs::write(root.join("structure.toml"), "").unwrap();
    let base = commit(&root, "triage.ts", "simple\n");
    let wisp_af6ed3fa = commit(&root, "triage.ts", "complex\n");
    assert_eq!(gates(&root, &base, &wisp_af6ed3fa, &state_home, &stub), ["code-health"]);
    let simple = commit(&root, "triage.ts", "simple again\n");
    let after = commit(&root, ".safe-push", "[land]\n\tafter = ** :: make slow-suite\n");
    assert_eq!(gates(&root, &simple, &after, &state_home, &stub), ["after-only reason"]);
    let reasoned = commit(&root, ".safe-push", "[land]\n\tafter = ** :: make slow-suite\n\tafter-only = reads the landed main's deployment :: make slow-suite\n");
    assert!(gates(&root, &simple, &reasoned, &state_home, &stub).is_empty());
}

const WISP_CI_CODE_HEALTH: &str = r#"name: CI
on:
  pull_request:
  push:
    branches-ignore: ["farm/**", "claude/**"]
jobs:
  code-health:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: npm i -g @ast-grep/cli@0.42.1
      - run: |
          "$CODE_HEALTH_NORTH_BIN" code-health gate "$base" HEAD
"#;

const WISP_CI_ANOTHER_GATE: &str = r#"name: CI
on:
  pull_request:
  push:
    branches-ignore: ["farm/**", "claude/**"]
jobs:
  code-health:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: npm i -g @ast-grep/cli@0.42.1
      - run: |
          "$CODE_HEALTH_NORTH_BIN" code-health gate "$base" HEAD
      - run: make another-gate
"#;

/// The push-to-main job that caught firn 710344e4's shellcheck after landing: it runs a discovered shell test on a clean runner, which no land check ran before the push.
const FIRN_TESTS_SHELLCHECK: &str = r#"name: Tests
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
jobs:
  tests:
    name: Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - run: nix build --no-link .#shellcheck
      - run: bash .github/scripts/land-checks.sh scripts
"#;

const RELEASE_ON_TAG: &str = "on:\n  push:\n    tags: ['v*']\njobs:\n  release:\n    runs-on: ubuntu-latest\n    steps:\n      - run: make release\n";

#[test]
fn a_lane_adding_wisps_push_to_main_code_health_job_with_no_check_and_no_reason_is_refused() {
    let (_dir, root, state_home, stub) = repository();
    let base = commit(&root, ".github/workflows/release-on-tag.yml", RELEASE_ON_TAG);
    let wisp_ci = commit(&root, ".github/workflows/ci.yml", WISP_CI_CODE_HEALTH);
    assert_eq!(gates(&root, &base, &wisp_ci, &state_home, &stub), ["after-only reason"]);
    let (code, text) = scanned(&root, &format!("{base}..{wisp_ci}"));
    assert_eq!(code, 1, "the scan refuses a lane whose push-to-main job runs nothing before landing");
    assert!(text.contains("`.github/workflows/ci.yml:code-health`"), "the scan names the job: {text}");
    let reasoned = commit(&root, ".safe-push", "[land]\n\tafter-only = north's code-health lane gate runs this check before landing :: .github/workflows/ci.yml:code-health\n");
    assert!(gates(&root, &base, &reasoned, &state_home, &stub).is_empty());
    assert_eq!(scanned(&root, &format!("{base}..{reasoned}")).0, 0, "an after-only reason satisfies the scan");
    at(&root, &wisp_ci);
    let checked = commit(&root, ".safe-push", "[land]\n\tcheck = ** :: npm i -g @ast-grep/cli@0.42.1\n\tcheck = ** :: \\\"$CODE_HEALTH_NORTH_BIN\\\" code-health gate \\\"$base\\\" HEAD\n");
    assert!(gates(&root, &base, &checked, &state_home, &stub).is_empty());
    assert_eq!(scanned(&root, &format!("{base}..{checked}")).0, 0, "a land.check for every run step satisfies the scan");
}

#[test]
fn a_lane_adding_firns_push_to_main_shellcheck_job_with_no_check_and_no_reason_is_refused() {
    let (_dir, root, state_home, stub) = repository();
    let base = commit(&root, ".github/workflows/release-on-tag.yml", RELEASE_ON_TAG);
    let firn_tests = commit(&root, ".github/workflows/tests.yml", FIRN_TESTS_SHELLCHECK);
    assert_eq!(gates(&root, &base, &firn_tests, &state_home, &stub), ["after-only reason"]);
    let reasoned = commit(&root, ".safe-push", "[land]\n\tafter-only = it runs every discovered shell test on a clean runner, where the changed ones run after the push as land checks :: .github/workflows/tests.yml:tests\n");
    assert!(gates(&root, &base, &reasoned, &state_home, &stub).is_empty());
}

#[test]
fn a_lane_that_changes_no_push_to_main_job_is_not_refused_for_the_one_it_inherits() {
    let (_dir, root, state_home, stub) = repository();
    let base = commit(&root, ".github/workflows/release-on-tag.yml", RELEASE_ON_TAG);
    let wisp_ci = commit(&root, ".github/workflows/ci.yml", WISP_CI_CODE_HEALTH);
    assert_eq!(gates(&root, &base, &wisp_ci, &state_home, &stub), ["after-only reason"]);
    let unrelated = commit(&root, "README.md", "a lane that changes only this file\n");
    assert!(gates(&root, &wisp_ci, &unrelated, &state_home, &stub).is_empty());
    assert_eq!(scanned(&root, &format!("{wisp_ci}..{unrelated}")).0, 0, "the scan does not re-judge a job the lane inherits");
    let changed = commit(&root, ".github/workflows/ci.yml", WISP_CI_ANOTHER_GATE);
    assert_eq!(gates(&root, &unrelated, &changed, &state_home, &stub), ["after-only reason"]);
    assert_eq!(scanned(&root, &format!("{unrelated}..{changed}")).0, 1, "the scan refuses a job the lane changes");
    at(&root, &changed);
    let reasoned = commit(&root, ".safe-push", "[land]\n\tafter-only = the job's extra gate has no before-landing copy :: .github/workflows/ci.yml:code-health\n");
    assert!(gates(&root, &unrelated, &reasoned, &state_home, &stub).is_empty());
}

fn scanned(root: &Path, range: &str) -> (u8, String) {
    let mut out = Vec::new();
    let code = crate::land::scans::ci_jobs::scan(root, range, &mut out);
    (code, String::from_utf8_lossy(&out).into_owned())
}
