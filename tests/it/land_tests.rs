use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const TEST: &str = "t.test.sh";
const BODY: &str = "status=0\nwhile read -r name outcome; do\n  if [ \"$outcome\" = pass ]; then echo \"PASS $name\"; else echo \"FAIL $name\"; status=1; fi\ndone <\"$OUTCOMES\"\nexit \"$status\"\n";

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
}

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

struct Check {
    passed: bool,
    stderr: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(root.path().join("home")).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join(TEST), BODY).unwrap();
        Fixture { root, repo }
    }

    fn commit(&self, label: &str) -> String {
        fs::write(self.repo.join("label.txt"), label).unwrap();
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-qm", label]);
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    fn check(&self, base: &str, head: &str, outcomes: &str) -> Check {
        git(&self.repo, &["checkout", "-q", head]);
        let file = self.root.path().join("outcomes");
        fs::write(&file, outcomes).unwrap();
        let Output { status, stderr, .. } = Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .args(["land-tests", TEST])
            .env("SAFE_PUSH_LAND_BASE", base)
            .env("SAFE_PUSH_LAND_HEAD", head)
            .env("OUTCOMES", &file)
            .env("HOME", self.root.path().join("home"))
            .env("XDG_STATE_HOME", self.state())
            .env_remove("ImageOS")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Check { passed: status.success(), stderr: String::from_utf8_lossy(&stderr).into_owned() }
    }

    fn recorded(&self, head: &str) -> String {
        let tree = git(&self.repo, &["rev-parse", &format!("{head}^{{tree}}")]);
        let directory = self.state().join("safe-push/case-results").join(tree);
        fs::read_dir(directory).unwrap().flatten().map(|entry| fs::read_to_string(entry.path()).unwrap()).collect()
    }

    fn red_main(&self) -> String {
        let root = self.commit("root");
        let main = self.commit("main");
        let first = self.check(&root, &main, "a fail\nb pass\n");
        assert!(!first.passed, "{}", first.stderr);
        main
    }
}

fn refusal(check: &Check) -> &str {
    check.stderr.lines().find(|line| line.contains("new failing cases")).unwrap_or_else(|| panic!("no refusal line:\n{}", check.stderr))
}

#[test]
fn land_tests_record_every_case_under_the_head_tree() {
    let fixture = Fixture::new();
    let main = fixture.red_main();
    let rows = fixture.recorded(&main);
    assert!(rows.contains("fail\tt.test.sh\ta\n") && rows.contains("pass\tt.test.sh\tb\n"), "{rows}");
}

#[test]
fn land_tests_land_a_lane_whose_only_red_case_is_red_on_the_base_and_print_it_quarantined() {
    let fixture = Fixture::new();
    let main = fixture.red_main();
    let lane = fixture.commit("lane");
    let check = fixture.check(&main, &lane, "a fail\nb pass\n");
    assert!(check.passed, "{}", check.stderr);
    assert!(check.stderr.contains("land check: quarantined t.test.sh a: red on main tree"), "{}", check.stderr);
    assert!(!check.stderr.contains("quarantined t.test.sh b"), "{}", check.stderr);
    assert!(fixture.recorded(&lane).contains("fail\tt.test.sh\ta\n"));
}

#[test]
fn land_tests_refuse_a_lane_red_case_beside_a_base_red_case_in_the_same_file_naming_only_it() {
    let fixture = Fixture::new();
    let main = fixture.red_main();
    let lane = fixture.commit("lane");
    let check = fixture.check(&main, &lane, "a fail\nb fail\n");
    assert!(!check.passed, "{}", check.stderr);
    assert!(refusal(&check).ends_with(": t.test.sh b"), "{}", check.stderr);
}

#[test]
fn land_tests_refuse_as_before_when_the_base_tree_has_no_recorded_results() {
    let fixture = Fixture::new();
    let root = fixture.commit("root");
    let lane = fixture.commit("lane");
    let check = fixture.check(&root, &lane, "a fail\nb pass\n");
    assert!(!check.passed, "{}", check.stderr);
    assert!(refusal(&check).ends_with(": t.test.sh a"), "{}", check.stderr);
    assert!(check.stderr.contains("has no recorded results for t.test.sh"), "{}", check.stderr);
    assert!(check.stderr.contains("land check: 1 of 1 tests failed: t.test.sh"), "{}", check.stderr);
}

#[test]
fn land_tests_never_count_a_pass_of_a_case_the_store_reports_flaky() {
    let fixture = Fixture::new();
    let root = fixture.commit("root");
    let green = fixture.check(&root, &root, "a pass\n");
    assert!(green.passed, "{}", green.stderr);
    let lane = fixture.commit("lane");
    assert!(!fixture.check(&root, &lane, "a fail\n").passed);
    let again = fixture.check(&root, &lane, "a pass\n");
    assert!(!again.passed, "a pass on a tree the store saw fail landed:\n{}", again.stderr);
    assert!(refusal(&again).ends_with(": t.test.sh a"), "{}", again.stderr);
}
