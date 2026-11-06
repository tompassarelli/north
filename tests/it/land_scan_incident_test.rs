use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GH_STUB: &str = r#"#!/bin/sh
printf '%s\n' "$*" >>"$STUB_DIR/gh-calls"
number=${2##*/}
[ -f "$STUB_DIR/labels-$number" ] || exit 1
cat "$STUB_DIR/labels-$number"
"#;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    base: String,
}

struct Scan {
    status: i32,
    stdout: String,
}

impl Fixture {
    fn new(origin: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["remote", "add", "origin", origin]);
        fs::write(repo.join("README"), "base\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        fs::create_dir_all(root.path().join("bin")).unwrap();
        shim::write_executable(&root.path().join("bin/gh"), GH_STUB);
        Self { root, repo, base }
    }

    fn label(&self, number: u64, labels: &str) {
        fs::write(self.root.path().join(format!("labels-{number}")), labels).unwrap();
    }

    fn lane(&self, subject: &str, files: &[(&str, &str)]) {
        git(&self.repo, &["reset", "-q", "--hard", &self.base]);
        for (path, text) in files {
            let path = self.repo.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-qm", subject]);
    }

    fn gh_calls(&self) -> usize {
        fs::read_to_string(self.root.path().join("gh-calls")).unwrap_or_default().lines().count()
    }

    fn scan_as(&self, name: &str, round: &str) -> Scan {
        let path = format!("{}:{}", self.root.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let output = Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .args(["land", "scan", name, &format!("{}..HEAD", self.base)])
            .env("PATH", path)
            .env("STUB_DIR", self.root.path())
            .env("HOME", self.root.path().join("home"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("NORTH_LAND_ROUND", round)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Scan { status: output.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&output.stdout).into_owned() }
    }

    fn scan(&self) -> Scan {
        self.scan_as("incident-test", "")
    }
}

const NORTH: &str = "git@github.com:tompassarelli/north.git";
const FIX: &str = "fn drain() {}\n";

#[test]
fn incident_fix_needs_a_test() {
    let fixture = Fixture::new(NORTH);
    fixture.label(7, "priority:now\nbug\n");
    for subject in ["Drain the precheck queue first (north#7)", "Drain the precheck queue first. Fixes #7"] {
        fixture.lane(subject, &[("crates/drain/src/drain.rs", FIX)]);
        let refused = fixture.scan();
        assert_eq!(refused.status, 1, "{subject}: {}", refused.stdout);
        assert_eq!(
            refused.stdout,
            "safe-push: ✗ this lane fixes bug north#7 but adds or changes no test; add a test that fails without the fix (a file under tests/ or test/, one named tests.*, *.test.*, *_test.* or *_tests.*, or a #[test]); NOT pushed.\n",
            "{subject}"
        );
    }
    let integrity = fixture.scan_as("test-integrity", "");
    assert_eq!(integrity.status, 1, "{}", integrity.stdout);
    assert!(integrity.stdout.contains("bug north#7"), "{}", integrity.stdout);
    let asked = fixture.gh_calls();
    let cases: [&[(&str, &str)]; 5] = [
        &[("docs/drain.md", "Drains first.\n"), ("README.md", "drain\n")],
        &[("crates/drain/src/drain.rs", FIX), ("tests/it/drain.rs", "#[test]\nfn drains() {}\n")],
        &[("crates/drain/src/drain.rs", FIX), ("crates/drain/src/drain_tests.rs", "fn helper() {}\n")],
        &[("crates/drain/src/drain.rs", FIX), ("crates/drain/src/tests.rs", "fn helper() {}\n")],
        &[("crates/drain/src/drain.rs", "fn drain() {}\n#[cfg(test)]\nmod t {\n    #[test]\n    fn drains() {}\n}\n")],
    ];
    for files in cases {
        fixture.lane("Drain the precheck queue first (north#7)", files);
        let passed = fixture.scan();
        assert_eq!(passed.status, 0, "{files:?}: {}", passed.stdout);
        assert!(passed.stdout.is_empty(), "{files:?}: {}", passed.stdout);
    }
    assert_eq!(fixture.gh_calls(), asked);
    fixture.label(8, "enhancement\n");
    fixture.lane("Add a drain verb (north#8)", &[("crates/drain/src/drain.rs", FIX)]);
    assert_eq!(fixture.scan().status, 0);
    let wisp = Fixture::new("git@github.com:tompassarelli/wisp.git");
    wisp.label(7, "bug\n");
    wisp.lane("Drain the queue. Fixes #7", &[("src/drain.rs", FIX)]);
    assert_eq!(wisp.scan().status, 0);
    assert_eq!(wisp.gh_calls(), 0);
    fixture.lane("Drain the precheck queue first (north#9)", &[("crates/drain/src/drain.rs", FIX)]);
    let unreachable = fixture.scan();
    assert_eq!(unreachable.status, 0, "{}", unreachable.stdout);
    assert_eq!(unreachable.stdout, "safe-push: ⚠ GitHub did not answer for the labels of north#9, so the incident-test scan passes it unchecked\n");
    let asked = fixture.gh_calls();
    fixture.lane("Drain the precheck queue first (north#7)", &[("crates/drain/src/drain.rs", FIX)]);
    for _ in 0..2 {
        assert_eq!(fixture.scan_as("incident-test", "r1").status, 1);
    }
    assert_eq!(fixture.gh_calls(), asked + 1);
    assert_eq!(fixture.scan_as("incident-test", "r2").status, 1);
    assert_eq!(fixture.gh_calls(), asked + 2);
    a_bug_fix_over_the_test_budget_lands_with_its_one_regression_test_and_both_gates_count_the_same_test_files();
}

fn a_bug_fix_over_the_test_budget_lands_with_its_one_regression_test_and_both_gates_count_the_same_test_files() {
    let fixture = Fixture::new("git@github.com:tompassarelli/firn.git");
    fixture.label(671, "bug\n");
    fixture.label(672, "enhancement\n");
    let ledger = fixture.root.path().join("state/safe-push/post-land.tsv");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    fs::write(&ledger, "1\tfirn\tabc\tgreen\t\tlane\t75\n".repeat(3)).unwrap();
    let unit = ("modules/land/default.nix", "{ Slice = \"agent.slice\"; }\n");
    for test in ["modules/land/tests/unit.test.sh", "modules/land/unit.test.sh", "modules/land/unit_test.sh", "src/tests.rs", "test/unit.sh", "tests/test_unit.py"] {
        fixture.lane("Run the lander in agent.slice (north#671)", &[unit, (test, "printf 'PASS: slice\\n'\n")]);
        for gate in ["incident-test", "test-admission"] {
            let scan = fixture.scan_as(gate, "");
            assert_eq!(scan.status, 0, "{test} {gate}: {}", scan.stdout);
        }
    }
    fixture.lane("Run the lander in agent.slice (north#671)", &[unit]);
    assert_eq!(fixture.scan_as("incident-test", "").status, 1);
    fixture.lane("Run the lander in agent.slice (north#671)", &[unit, ("tests/a.test.sh", "x\n"), ("tests/b.test.sh", "x\n")]);
    let two = fixture.scan_as("test-admission", "");
    assert_eq!(two.status, 1, "{}", two.stdout);
    assert!(two.stdout.contains("unless it pins a listed contract or is the one regression test of each bug the lane fixes (north#671)"), "{}", two.stdout);
    fixture.lane("Add a lander slice (north#672)", &[unit, ("tests/a.test.sh", "x\n")]);
    let feature = fixture.scan_as("test-admission", "");
    assert_eq!(feature.status, 1, "{}", feature.stdout);
    assert!(feature.stdout.contains("(a bug-labelled north issue its commit subject cites)"), "{}", feature.stdout);
}
