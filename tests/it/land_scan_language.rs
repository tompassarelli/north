use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NORTH_STUB: &str = r#"printf '%s\n' "$*" >>"$STUB_TRACE"
if [ "${2:-}" = --help ]; then
  case " ${STUB_LACKS:-} " in *" $1 "*) printf "error: unrecognized command '%s'\n" "$1" >&2; exit 2 ;; esac
  [ "$1" != bin-references ] || exit "${STUB_BIN_HELP_STATUS:-0}"
  exit 0
fi
if [ "$1" = language-gate ] && [ -n "${STUB_DENY_HEAD:-}" ] && [ "$7" = "$STUB_DENY_HEAD" ]; then
  printf '%s: Python files are forbidden\n' "$7"
  exit 1
fi
if [ "$1" = "${STUB_REFUSE:-}" ]; then
  printf '%s refused\n' "$1"
  exit "${STUB_REFUSE_STATUS:-1}"
fi
exit 0
"#;

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    base: String,
    head: String,
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

fn commit(repo: &Path, message: &str) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", message]);
    git(repo, &["rev-parse", "HEAD"])
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("bin")).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join("bin/old-tool"), "#!/bin/sh\n").unwrap();
        fs::write(repo.join("file.txt"), "base\n").unwrap();
        let base = commit(&repo, "base");
        fs::write(repo.join("file.txt"), "changed\n").unwrap();
        let head = commit(&repo, "head");
        shim::write_executable(&root.path().join("north"), format!("#!/usr/bin/env bash\nset -u\n{NORTH_STUB}"));
        let repo = PathBuf::from(git(&repo, &["rev-parse", "--show-toplevel"]));
        Self { root, repo, base, head }
    }

    fn delete_bin(&mut self) {
        fs::remove_file(self.repo.join("bin/old-tool")).unwrap();
        self.head = commit(&self.repo, "drop old-tool");
    }

    fn scan(&self, base: &str, head: &str, extra: &[&str], environment: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .current_dir(&self.repo)
            .args(["land", "scan", "language", "--base", base, "--head", head])
            .args(extra)
            .env("NORTH_LAND_NORTH", self.root.path().join("north"))
            .env("STUB_TRACE", self.root.path().join("trace"))
            .stdin(Stdio::null());
        for (key, value) in environment {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    fn run(&self, environment: &[(&str, &str)]) -> Output {
        self.scan(&self.base, &self.head, &[], environment)
    }

    fn gate_call(&self, subcommand: &str, base: &str, head: &str) -> String {
        format!("{subcommand} --repo {} --base {base} --head {head}", self.repo.display())
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.root.path().join("trace")).unwrap_or_default().lines().filter(|line| !line.ends_with("--help")).map(str::to_owned).collect()
    }
}

fn status(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn language_gate_boundary() {
    let fixture = Fixture::new();
    let passed = fixture.run(&[]);
    assert_eq!(status(&passed), 0, "{}", stderr(&passed));
    let gates: Vec<String> = ["language-gate", "build-cache-gate", "runtime-floor-gate"].iter().map(|gate| fixture.gate_call(gate, &fixture.base, &fixture.head)).collect();
    assert_eq!(fixture.calls(), gates);
    fs::remove_file(fixture.root.path().join("trace")).unwrap();
    let denied = fixture.run(&[("STUB_DENY_HEAD", &fixture.head)]);
    assert_eq!(status(&denied), 1);
    assert!(stdout(&denied).contains(&format!("{}: Python files are forbidden", fixture.head)), "{}", stdout(&denied));
    assert!(stderr(&denied).contains("safe-push: language gate refused the landing; NOT pushed."), "{}", stderr(&denied));
    assert_eq!(fixture.calls(), [fixture.gate_call("language-gate", &fixture.base, &fixture.head)]);
}

#[test]
fn language_mode_refusal() {
    let fixture = Fixture::new();
    for (gate, label, remaining) in [("build-cache-gate", "build-cache gate", 2), ("runtime-floor-gate", "runtime-floor gate", 3)] {
        let _ = fs::remove_file(fixture.root.path().join("trace"));
        let refused = fixture.run(&[("STUB_REFUSE", gate), ("STUB_REFUSE_STATUS", "7")]);
        assert_eq!(status(&refused), 7, "{gate}");
        assert!(stdout(&refused).contains(&format!("{gate} refused")), "{}", stdout(&refused));
        assert!(stderr(&refused).contains(&format!("safe-push: {label} refused the landing; NOT pushed.")), "{}", stderr(&refused));
        assert_eq!(fixture.calls().len(), remaining, "{gate}: {:?}", fixture.calls());
    }
}

#[test]
fn language_mode_runtime_lacks_gate() {
    let fixture = Fixture::new();
    let safe_push = fixture.repo.join("bin/safe-push");
    let safe_push = safe_push.to_str().unwrap();
    let revision = git(&fixture.repo, &["rev-parse", "--short=12", "HEAD"]);
    for gate in ["language-gate", "build-cache-gate"] {
        let _ = fs::remove_file(fixture.root.path().join("trace"));
        let lacking = fixture.scan(&fixture.base, &fixture.head, &["--safe-push", safe_push], &[("STUB_LACKS", gate)]);
        assert_eq!(status(&lacking), 69, "{gate}");
        assert!(
            stderr(&lacking).contains(&format!("safe-push: this host's north runtime lacks north {gate}, which safe-push {revision} calls; run north runtime update to {revision} or later; NOT pushed.")),
            "{}",
            stderr(&lacking)
        );
        assert!(fixture.calls().is_empty(), "{gate}: {:?}", fixture.calls());
    }
    let unnamed = fixture.scan(&fixture.base, &fixture.head, &["--safe-push", "/nonexistent/safe-push"], &[("STUB_LACKS", "language-gate")]);
    assert!(stderr(&unnamed).contains("which safe-push this safe-push calls"), "{}", stderr(&unnamed));
}

#[test]
fn language_mode_runtime_floor_optional() {
    let fixture = Fixture::new();
    let passed = fixture.run(&[("STUB_LACKS", "runtime-floor-gate")]);
    assert_eq!(status(&passed), 0, "{}", stderr(&passed));
    assert!(stderr(&passed).is_empty(), "{}", stderr(&passed));
    assert_eq!(fixture.calls().len(), 2, "{:?}", fixture.calls());
}

#[test]
fn language_initial() {
    let fixture = Fixture::new();
    let empty_tree = git(&fixture.repo, &["hash-object", "-t", "tree", "/dev/null"]);
    let denied = fixture.scan("", &fixture.head, &[], &[("STUB_DENY_HEAD", &fixture.head)]);
    assert_eq!(status(&denied), 1);
    assert_eq!(fixture.calls(), [fixture.gate_call("language-gate", &empty_tree, &fixture.head)]);
}

#[test]
fn train_language_denied() {
    let fixture = Fixture::new();
    let denied = fixture.run(&[("STUB_REFUSE", "language-gate"), ("STUB_REFUSE_STATUS", "5")]);
    assert_eq!(status(&denied), 5);
    assert!(stderr(&denied).contains("safe-push: language gate refused the landing; NOT pushed."), "{}", stderr(&denied));
    assert_eq!(fixture.calls().len(), 1, "{:?}", fixture.calls());
}

#[test]
fn train_language_missing() {
    let fixture = Fixture::new();
    let missing = fixture.run(&[("NORTH_LAND_NORTH", "/nonexistent/north")]);
    assert_eq!(status(&missing), 127);
    assert!(stderr(&missing).contains("safe-push: language gate refused the landing; NOT pushed."), "{}", stderr(&missing));
}

#[test]
fn train_captured_candidate() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("new.py"), "print()\n").unwrap();
    let candidate = commit(&fixture.repo, "python");
    git(&fixture.repo, &["reset", "-q", "--hard", &fixture.base]);
    let denied = fixture.scan(&fixture.base, &candidate, &[], &[("STUB_DENY_HEAD", &candidate)]);
    assert_eq!(status(&denied), 1);
    assert!(stdout(&denied).contains(&format!("{candidate}: Python files are forbidden")), "{}", stdout(&denied));
    assert_eq!(fixture.calls(), [fixture.gate_call("language-gate", &fixture.base, &candidate)]);
}

#[test]
fn bin_reference_gate() {
    let mut fixture = Fixture::new();
    let north = ["--origin", "git@github.com:tompassarelli/north.git"];
    let unchanged = fixture.scan(&fixture.base, &fixture.head, &north, &[]);
    assert_eq!(status(&unchanged), 0);
    assert!(!fixture.calls().iter().any(|call| call.starts_with("bin-references")), "{:?}", fixture.calls());
    fixture.delete_bin();
    for origin in ["https://github.com/tompassarelli/wisp.git", ""] {
        let _ = fs::remove_file(fixture.root.path().join("trace"));
        let other = fixture.scan(&fixture.base, &fixture.head, &["--origin", origin], &[]);
        assert_eq!(status(&other), 0);
        assert_eq!(fixture.calls().len(), 3, "{origin}: {:?}", fixture.calls());
    }
    for origin in [north[1], "https://github.com/tompassarelli/north"] {
        let _ = fs::remove_file(fixture.root.path().join("trace"));
        let refused = fixture.scan(&fixture.base, &fixture.head, &["--origin", origin], &[("STUB_REFUSE", "bin-references"), ("STUB_REFUSE_STATUS", "4")]);
        assert_eq!(status(&refused), 4, "{origin}");
        assert_eq!(fixture.calls().last().unwrap(), &fixture.gate_call("bin-references", &fixture.base, &fixture.head));
    }
    let lacking = fixture.scan(&fixture.base, &fixture.head, &north, &[("STUB_BIN_HELP_STATUS", "2")]);
    assert_eq!(status(&lacking), 1);
    assert!(
        stderr(&lacking).contains("safe-push: this host's north runtime lacks north bin-references, so a deleted bin/ entry cannot be checked against deployed units; run north runtime update; NOT pushed."),
        "{}",
        stderr(&lacking)
    );
}
