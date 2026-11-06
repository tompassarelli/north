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

fn admission_scan(fixture: &Fixture) -> Scan {
    let output = fixture.north(&fixture.repo, &["land", "scan", "test-admission", &format!("{}..{}", fixture.base, fixture.head())]);
    Scan { status: output.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&output.stdout).into_owned() }
}

fn trail(fixture: &Fixture, trailer: &str) {
    git(&fixture.repo, &["commit", "-q", "--amend", "-m", "add a test", "-m", trailer]);
}

fn verification_takes(fixture: &Fixture, seconds: u64) {
    let green = format!("1\tremote\tabc\tgreen\t\tlane\t{seconds}\n").repeat(3);
    let red = "1\tremote\tdef\tred-main\tcheck\tlane\t900\n".repeat(4);
    write(&fixture.root.path().join("state/safe-push/post-land.tsv"), &(green + &red));
}

#[test]
fn added_tests_are_refused_under_an_unlisted_contract_or_over_the_verification_budget_without_a_deletion() {
    let mut fixture = Fixture::new("remote.git");
    write(&fixture.repo.join(".safe-push"), "[test-admission]\n\tcontract = lua-determinism :: Lua evaluation order\n");
    write(&fixture.repo.join("src/lib.rs"), "pub fn retry() {}\n");
    write(&fixture.repo.join("tests/old.test.ts"), "{}\n");
    fixture.commit(&["library"]);
    fixture.mark_base();
    write(&fixture.repo.join("src/retry_tests.rs"), RUST_TEST);
    fixture.commit(&["add the retry test"]);
    assert_eq!(admission_scan(&fixture).status, 0, "no claim and no measured budget");
    for (claim, status) in [("retries; category: lua-determinism", 0), ("retries; category: netcode-replay", 1), ("other; category: lua-determinism", 1)] {
        trail(&fixture, &format!("Test-contract: {claim}"));
        let scan = admission_scan(&fixture);
        assert_eq!(scan.status, status, "{claim}: {}", scan.stdout);
    }
    trail(&fixture, "No contract claimed.");
    for (seconds, status) in [(20, 0), (21, 1)] {
        verification_takes(&fixture, seconds);
        let scan = admission_scan(&fixture);
        assert_eq!(scan.status, status, "{seconds} s: {}", scan.stdout);
    }
    trail(&fixture, "Test-contract: retries; category: lua-determinism");
    let contracted = admission_scan(&fixture);
    assert_eq!(contracted.status, 0, "{}", contracted.stdout);
    for path in ["tests/fixtures/case.json", "tests/with space/case.json", "web/retry.test.ts", "scripts/retry_test.lua", "src/testdata/snapshots/retry.txt"] {
        fixture.reset();
        write(&fixture.repo.join(path), "{}\n");
        fixture.commit(&["add a test file"]);
        let scan = admission_scan(&fixture);
        assert_eq!(scan.status, 1, "{path}: {}", scan.stdout);
    }
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), &format!("pub fn retry() {{}}\n{RUST_TEST}"));
    fixture.commit(&["add a test to an existing source file"]);
    assert_eq!(admission_scan(&fixture).status, 1);
    fixture.reset();
    fs::remove_file(fixture.repo.join("tests/old.test.ts")).unwrap();
    write(&fixture.repo.join("tests/new.test.ts"), "test(\"retries\");\n");
    fixture.commit(&["replace a test"]);
    let replaced = admission_scan(&fixture);
    assert_eq!(replaced.status, 0, "{}", replaced.stdout);
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), &format!("pub fn retry() {{}}\n{RUST_TEST}"));
    fixture.commit(&["a source file with an inline test"]);
    fixture.mark_base();
    write(&fixture.repo.join("src/lib.rs"), "pub fn retry() {}\n");
    write(&fixture.repo.join("src/retry_tests.rs"), RUST_TEST);
    fixture.commit(&["move the test out of the source file"]);
    assert_eq!(admission_scan(&fixture).status, 0);
}

const STUB_RUNNER: &str = "printf '%s\\n' \"$MUTANT_TREES\" | while read -r tree; do\n  for name in $MUTANT_TESTS; do\n    if [ \"$name\" = guards ] && ! grep -q fixed \"$tree/src/lib.rs\"; then v=fail; else v=pass; fi\n    printf '%s\\t%s\\n' \"$v\" \"$name\" >>\"$tree/.mutant-results\"\n  done\ndone\n";
const GUARDS: &str = "#[test]\nfn guards() {}\n";
const IDLE: &str = "#[test]\nfn idle() {}\n";

fn fail_to_pass(fixture: &Fixture) -> Scan {
    let range = format!("{}..{}", fixture.base, fixture.head());
    let output = fixture.north(&fixture.repo, &["land", "scan", "test-admission", "run", &range]);
    Scan { status: output.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&output.stdout).into_owned() }
}

#[test]
fn an_added_test_that_passes_without_the_fix_is_refused_after_the_push() {
    let mut fixture = Fixture::new("remote.git");
    write(&fixture.repo.join(".safe-push"), "[mutant]\n\trun = sh harness/runner.sh\n");
    write(&fixture.repo.join("harness/runner.sh"), STUB_RUNNER);
    write(&fixture.repo.join("src/lib.rs"), "pub fn f() {}\nfn g() {}\n");
    fixture.commit(&["library"]);
    fixture.mark_base();
    write(&fixture.repo.join("src/lib.rs"), "pub fn f() { /* fixed */ }\nfn g() {}\n");
    write(&fixture.repo.join("tests/bug.rs"), &format!("{GUARDS}{IDLE}"));
    fixture.commit(&["fix f"]);
    let idle = fail_to_pass(&fixture);
    assert_eq!(idle.status, 1, "{}", idle.stdout);
    assert!(idle.stdout.contains("so none guards the change: idle;"), "{}", idle.stdout);
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), "pub fn f() { /* fixed */ }\nfn g() {}\n");
    write(&fixture.repo.join("tests/bug.rs"), GUARDS);
    fixture.commit(&["fix f"]);
    let guarded = fail_to_pass(&fixture);
    assert_eq!(guarded.status, 0, "{}", guarded.stdout);
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), &format!("pub fn f() {{ /* fixed */ }}\nfn g() {{}}\n{IDLE}{GUARDS}"));
    fixture.commit(&["fix f with inline tests"]);
    let inline = fail_to_pass(&fixture);
    assert_eq!(inline.status, 1, "{}", inline.stdout);
    assert!(inline.stdout.contains("so none guards the change: idle;"), "{}", inline.stdout);
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), "pub fn f() { /* fixed */ }\nfn g() {}\n");
    write(&fixture.repo.join("tests/bug.rs"), IDLE);
    fixture.commit(&["refactor f", "Behavior: unchanged"]);
    assert_eq!(fail_to_pass(&fixture).status, 0);
    fixture.reset();
    write(&fixture.repo.join("tests/bug.rs"), IDLE);
    fixture.commit(&["pin f"]);
    assert_eq!(fail_to_pass(&fixture).status, 0);
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), "pub fn f() {}\nfn g() { /* fixed */ }\n#[test]\nfn guards() {}\n");
    fixture.commit(&["fix g next to its test"]);
    let adjacent = fail_to_pass(&fixture);
    assert_eq!(adjacent.status, 0, "{}", adjacent.stdout);
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), "pub fn f() {}\n#[test]\nfn guards() {}\nfn g() { /* fixed */ }\n");
    fixture.commit(&["fix g after its test"]);
    let after = fail_to_pass(&fixture);
    assert_eq!(after.status, 0, "{}", after.stdout);
    fixture.reset();
    write(&fixture.repo.join("src/lib.rs"), &format!("pub fn f() {{}}\nfn g() {{}}\n{IDLE}"));
    fixture.commit(&["pin f inline"]);
    assert_eq!(fail_to_pass(&fixture).status, 0);
}
