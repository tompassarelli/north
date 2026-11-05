use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const MUTANT_MARKER: &str = concat!("MUT", "ANT:r1");
const SKIP: &str = concat!("it.sk", "ip(");

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
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
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
        git(root.path(), &["init", "-q", "--bare", origin]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                root.path().join(origin).to_str().unwrap(),
            ],
        );
        write(&repo.join("content.txt"), "initial\n");
        git(&repo, &["add", "content.txt"]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(&repo, &["push", "-q", "origin", "HEAD:main"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        Self { root, repo, base }
    }

    fn commit(&self, message: &[&str]) -> String {
        git(&self.repo, &["add", "-A", "."]);
        let mut arguments = vec!["commit", "-q", "--allow-empty"];
        for paragraph in message {
            arguments.extend(["-m", paragraph]);
        }
        git(&self.repo, &arguments);
        self.head()
    }

    fn head(&self) -> String {
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn mark_base(&mut self) {
        git(&self.repo, &["push", "-q", "origin", "HEAD:main"]);
        self.base = self.head();
    }

    fn reset(&self) {
        git(&self.repo, &["reset", "-q", "--hard", &self.base]);
        git(&self.repo, &["clean", "-qfd"]);
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn north(&self, directory: &Path, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(directory)
            .args(arguments)
            .env("HOME", self.home())
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn scan_range(&self, name: &str, range: &str) -> Scan {
        let output = self.north(&self.repo, &["land", "scan", name, range]);
        Scan {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        }
    }

    fn scan(&self, name: &str) -> Scan {
        self.scan_range(name, &format!("{}..{}", self.base, self.head()))
    }
}

#[test]
fn test_integrity() {
    let mut fixture = Fixture::new("remote.git");
    write(&fixture.repo.join("retry.test.ts"), "");
    fixture.commit(&["existing retry test file"]);
    fixture.mark_base();
    write(
        &fixture.repo.join("product.py"),
        &format!("answer = 41  # {MUTANT_MARKER}\n"),
    );
    fixture.commit(&["product mutant", "Test-change: none"]);
    let mutant = fixture.scan("test-integrity");
    assert_eq!(mutant.status, 1);
    assert_eq!(
        mutant.stdout,
        format!(
            "safe-push: ✗ product.py adds a mutation marker: answer = 41  # {MUTANT_MARKER} — mutants stay in scratch copies; NOT pushed.\n"
        )
    );
    for trailer in [None, Some("Test-change: the retry rule was removed")] {
        fixture.reset();
        write(
            &fixture.repo.join("retry.test.ts"),
            &format!("{SKIP}\"retries\", () => {{}})\n"),
        );
        let mut message = vec!["skip retry test"];
        message.extend(trailer);
        fixture.commit(&message);
        let scan = fixture.scan("test-integrity");
        match trailer {
            None => {
                assert_eq!(scan.status, 1);
                assert_eq!(
                    scan.stdout,
                    format!(
                        "safe-push: ✗ retry.test.ts adds a skip or focus: {SKIP}\"retries\", () => {{}}) — name the changed requirement in a Test-change: trailer; NOT pushed.\n"
                    )
                );
            }
            Some(_) => assert_eq!(scan.status, 0, "{}", scan.stdout),
        }
    }
    fixture.reset();
    write(
        &fixture.repo.join("src/retry.rs"),
        &format!("{SKIP}\"not a test path\")\n"),
    );
    fixture.commit(&["skip outside tests"]);
    assert_eq!(fixture.scan("test-integrity").status, 0);
}

const RUST_TEST: &str = concat!("#[te", "st]\nfn retries() {}\n");

fn admission_scan(fixture: &Fixture, session: &str) -> Scan {
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .current_dir(&fixture.repo)
        .args(["land", "scan", "test-admission", &format!("{}..{}", fixture.base, fixture.head())])
        .env("HOME", fixture.home())
        .env("NORTH_FLEET_SESSION", session)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Scan { status: output.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&output.stdout).into_owned() }
}

fn admission_id(fixture: &Fixture) -> String {
    let range = format!("{}..{}", fixture.base, fixture.head());
    let output = fixture.north(&fixture.repo, &["land", "scan", "test-admission", "id", &range]);
    String::from_utf8_lossy(&output.stdout).lines().next().unwrap().strip_prefix("patch-id ").unwrap().to_owned()
}

fn amend(fixture: &Fixture, id: &str, reviewer: &str) {
    git(&fixture.repo, &["add", "-A", "."]);
    let trailer = format!("Test-review: keep 1 of 1; reviewer: {reviewer}; patch-id {id}");
    git(
        &fixture.repo,
        &["commit", "-q", "--amend", "-m", "add the retry test", "-m", "retries: kept, it reproduces a named bug", "-m", &trailer],
    );
}

#[test]
fn tests_are_admitted_only_with_an_independent_review_of_their_final_hunks() {
    let mut fixture = Fixture::new("remote.git");
    write(&fixture.repo.join("src/lib.rs"), "pub fn retry() {}\n");
    fixture.commit(&["library"]);
    fixture.mark_base();
    write(&fixture.repo.join("src/retry_tests.rs"), RUST_TEST);
    fixture.commit(&["add the retry test"]);
    let unreviewed = admission_scan(&fixture, "author-1");
    assert_eq!(unreviewed.status, 1);
    let id = admission_id(&fixture);
    amend(&fixture, &id, "reviewer-1");
    assert_eq!(admission_scan(&fixture, "author-1").status, 0);
    let own = admission_scan(&fixture, "reviewer-1");
    assert_eq!(own.status, 1);
    assert!(own.stdout.contains("reviewer reviewer-1 is the session that landed it"), "{}", own.stdout);
    write(&fixture.repo.join("src/retry_tests.rs"), &format!("{RUST_TEST}fn extra() {{}}\n"));
    amend(&fixture, &id, "reviewer-1");
    let edited = admission_scan(&fixture, "author-1");
    assert_eq!(edited.status, 1);
    assert!(edited.stdout.contains("the review never saw the final test hunks"), "{}", edited.stdout);
    for path in ["tests/fixtures/case.json", "tests/with space/case.json", "web/retry.test.ts", "scripts/retry_test.lua", "src/testdata/snapshots/retry.txt"] {
        fixture.reset();
        write(&fixture.repo.join(path), "{}\n");
        fixture.commit(&["add a test file"]);
        let scan = admission_scan(&fixture, "author-1");
        assert_eq!(scan.status, 1, "{path}: {}", scan.stdout);
    }
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), &format!("pub fn retry() {{}}\n{RUST_TEST}"));
    fixture.commit(&["add a test to an existing source file"]);
    assert_eq!(admission_scan(&fixture, "author-1").status, 1);
    fixture.reset();
    write(&fixture.repo.join("src/retry_tests.rs"), RUST_TEST);
    fixture.commit(&["add the retry test"]);
    let id = admission_id(&fixture);
    for counts in ["keep 0 of 1", "keep 1 of 2"] {
        git(&fixture.repo, &["commit", "-q", "--amend", "-m", "add the retry test", "-m", &format!("Test-review: {counts}; reviewer: reviewer-1; patch-id {id}")]);
        assert_eq!(admission_scan(&fixture, "author-1").status, 1, "{counts}");
    }
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), &format!("pub fn retry() {{}}\n{RUST_TEST}"));
    fixture.commit(&["a source file with an inline test"]);
    fixture.mark_base();
    write(&fixture.repo.join("src/lib.rs"), "pub fn retry() {}\n");
    write(&fixture.repo.join("src/retry_tests.rs"), RUST_TEST);
    fixture.commit(&["move the test out of the source file"]);
    assert_eq!(admission_scan(&fixture, "author-1").status, 0);
}
