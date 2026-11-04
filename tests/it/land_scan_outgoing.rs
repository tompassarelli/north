use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const PUSHER: &str = "safe-push-test@example.invalid";
const PRIVATE: &str = "someone@private.example";
const SECRET_TERM: &str = "zebra-quokka-7731";

const DENYLIST: &str = "title = \"safe-push test deny-list\"\n[[rules]]\nid = \"fake-private-term\"\ndescription = \"fake private identity term\"\nregex = '''zebra-quokka-7731'''\n";

const GITLEAKS_STUB: &str = "#!/usr/bin/env bash\nset -u\ncase \"$*\" in *--config*) exec \"$STUB_REAL_GITLEAKS\" \"$@\" ;; esac\nprintf '%s\\n' \"$*\" >>\"$STUB_TRACE\"\nif [ -n \"${STUB_MUTATION:-}\" ] && [ ! -e \"$STUB_TRACE.mutated\" ]; then : >\"$STUB_TRACE.mutated\"; eval \"$STUB_MUTATION\"; fi\nexit \"${STUB_GITLEAKS_STATUS:-0}\"\n";

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    base: String,
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_COMMITTER_NAME", "t")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_EMAIL")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, message: &str, environment: &[(&str, &str)]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_COMMITTER_NAME", "t")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_EMAIL")
        .envs(environment.iter().copied())
        .args(["commit", "--allow-empty", "-qm", message])
        .output()
        .unwrap();
    assert!(output.status.success(), "commit: {}", String::from_utf8_lossy(&output.stderr));
    git(repo, &["rev-parse", "HEAD"])
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let remote = root.path().join("remote.git");
        fs::create_dir_all(&repo).unwrap();
        git(root.path(), &["init", "-q", "--bare", "remote.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "safe-push-test"]);
        git(&repo, &["config", "user.email", PUSHER]);
        git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        fs::write(repo.join("content.txt"), "initial\n").unwrap();
        git(&repo, &["add", "content.txt"]);
        git(&repo, &["commit", "-qm", "initial"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["push", "-q", "origin", "HEAD:main"]);
        let stubs = root.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        shim::write_executable(&stubs.join("gh"), "#!/bin/sh\nprintf 'PUBLIC\\n'\n");
        fs::write(root.path().join("denylist.toml"), DENYLIST).unwrap();
        Self { root, repo, base }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn range(&self) -> String {
        format!("{}..HEAD", self.base)
    }

    fn scan(&self, arguments: &[&str], gitleaks: Option<&str>, environment: &[(&str, &str)]) -> Output {
        let mut path = self.path("stubs").display().to_string();
        if let Some(body) = gitleaks {
            let bin = self.path("gitleaks-bin");
            fs::create_dir_all(&bin).unwrap();
            shim::write_executable(&bin.join("gitleaks"), body);
            path = format!("{}:{path}", bin.display());
        }
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .args(["land", "scan"])
            .args(arguments)
            .env("PATH", format!("{path}:{}", std::env::var("PATH").unwrap_or_default()))
            .env("SAFE_PUSH_DENYLIST", self.path("denylist.toml"))
            .env("SAFE_PUSH_SOUTH", self.path("no-south"))
            .env("STUB_REAL_GITLEAKS", real_gitleaks())
            .env("STUB_TRACE", self.path("trace"))
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("GIT_COMMITTER_EMAIL")
            .envs(environment.iter().copied())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn trace(&self) -> String {
        fs::read_to_string(self.path("trace")).unwrap_or_default()
    }
}

fn real_gitleaks() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|dir| dir.join("gitleaks")).find(|candidate| candidate.is_file()).expect("gitleaks on PATH")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn leak() -> [(&'static str, &'static str); 1] {
    [("STUB_GITLEAKS_STATUS", "1")]
}

#[test]
fn denylist_public_finding() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("identity.txt"), format!("owner: {SECRET_TERM}\n")).unwrap();
    git(&fixture.repo, &["add", "identity.txt"]);
    git(&fixture.repo, &["commit", "-qm", "add identity"]);
    let head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let output = fixture.scan(&["denylist", &fixture.base, &head, "main"], None, &[]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("fake-private-term"), "{text}");
    assert!(text.contains("identity.txt"), "{text}");
    assert!(text.contains("PRIVATE VALUE for a PUBLIC repository matched the deny-list"), "{text}");
    assert!(!text.contains(SECRET_TERM), "the refusal printed the matched value: {text}");
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET_TERM));
    assert_eq!(git(&fixture.repo, &["--git-dir", fixture.path("remote.git").to_str().unwrap(), "rev-parse", "main"]), fixture.base);
}

#[test]
fn denylist_clean_and_private_repositories_pass() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("identity.txt"), format!("owner: {SECRET_TERM}\n")).unwrap();
    git(&fixture.repo, &["add", "identity.txt"]);
    git(&fixture.repo, &["commit", "-qm", "add identity"]);
    shim::write_executable(&fixture.path("stubs/gh"), "#!/bin/sh\nprintf 'PRIVATE\\n'\n");
    let private = fixture.scan(&["denylist", &fixture.base, "HEAD", "main"], None, &[]);
    assert_eq!(private.status.code(), Some(0), "{}", stdout(&private));
    shim::write_executable(&fixture.path("stubs/gh"), "#!/bin/sh\nprintf 'PUBLIC\\n'\n");
    let clean = fixture.scan(&["denylist", &fixture.base, &fixture.base, "main"], None, &[]);
    assert_eq!(clean.status.code(), Some(0), "{}", stdout(&clean));
}

#[test]
fn email_to_main() {
    let fixture = Fixture::new();
    let sha = commit(&fixture.repo, "private author for main", &[("GIT_AUTHOR_EMAIL", PRIVATE)]);
    let output = fixture.scan(&["commit-email", &fixture.range()], None, &[]);
    let short = git(&fixture.repo, &["rev-parse", "--short", &sha]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        format!("safe-push: commit {short} author email {PRIVATE} is not {PUSHER}; re-author with git commit --amend --reset-author (or rebase --exec) before pushing; NOT pushed.\n")
    );
    commit(&fixture.repo, "clean follow-up", &[]);
    git(&fixture.repo, &["config", "--add", "safe-push.allowEmail", PRIVATE]);
    let allowed = fixture.scan(&["commit-email", &fixture.range()], None, &[]);
    assert_eq!(allowed.status.code(), Some(0), "{}", stdout(&allowed));
}

fn race(mutation: &str) {
    let fixture = Fixture::new();
    commit(&fixture.repo, "outgoing", &[]);
    let head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let other = fixture.path("other-git");
    fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    let other_git = other.join(".git").to_str().unwrap().to_owned();
    let remote = fixture.path("remote.git");
    let environment = [
        ("STUB_MUTATION", mutation),
        ("STUB_REMOTE", remote.to_str().unwrap()),
        ("STUB_BASE", fixture.base.as_str()),
        ("STUB_OTHER_GIT", other_git.as_str()),
    ];
    let range = format!("{}..{head}", fixture.base);
    let output = fixture.scan(&["secrets", &range], Some(GITLEAKS_STUB), &environment);
    assert_eq!(output.status.code(), Some(0), "{}{}", stdout(&output), String::from_utf8_lossy(&output.stderr));
    assert_eq!(fixture.trace(), format!("detect --no-banner --redact --log-opts={range}\n"));
    assert!(fixture.path("trace.mutated").exists(), "the race was never planted");
    let leaking = fixture.scan(&["secrets", &range], Some(GITLEAKS_STUB), &leak());
    assert_eq!(leaking.status.code(), Some(1));
}

#[test]
fn race_branch() {
    race("git switch -q -c raced-branch");
}

#[test]
fn race_commit() {
    race("git commit --allow-empty -qm 'raced commit'");
}

#[test]
fn race_destination() {
    race("git --git-dir=\"$STUB_REMOTE\" update-ref refs/heads/main \"$STUB_BASE\"");
}

#[test]
fn race_worktree() {
    race("mv .git .git.safe-push-original && printf 'gitdir: %s\\n' \"$STUB_OTHER_GIT\" >.git");
}

#[test]
fn native_ref() {
    let fixture = Fixture::new();
    commit(&fixture.repo, "query scroll target for ops#17", &[]);
    let output = fixture.scan(&["native-ref", "--range", &fixture.range()], None, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("names a native NorthThread item"), "{}", stdout(&output));

    let fixture = Fixture::new();
    commit(&fixture.repo, "troops#3 and prefix-ops# are no item", &[]);
    let clean = fixture.scan(&["native-ref", "--range", &fixture.range(), "--", "claude/clean-work", "main"], None, &[]);
    assert_eq!(clean.status.code(), Some(0), "{}", stdout(&clean));
    let branch = fixture.scan(&["native-ref", "--range", &fixture.range(), "--", "claude/travel#4-work", "main"], None, &[]);
    assert_eq!(branch.status.code(), Some(1));
}

#[test]
fn all_at_once() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("identity.txt"), format!("owner: {SECRET_TERM}\n")).unwrap();
    git(&fixture.repo, &["add", "identity.txt"]);
    commit(&fixture.repo, "ships ops#9 beside a leak", &[("GIT_AUTHOR_EMAIL", PRIVATE)]);
    let range = fixture.range();
    let head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let refusals = [
        fixture.scan(&["native-ref", "--range", &range], None, &[]),
        fixture.scan(&["secrets", &range], Some(GITLEAKS_STUB), &leak()),
        fixture.scan(&["denylist", &fixture.base, &head, "main"], None, &[]),
        fixture.scan(&["commit-email", &range], None, &[]),
    ];
    for (index, output) in refusals.iter().enumerate() {
        assert_eq!(output.status.code(), Some(1), "scan {index} did not refuse on its own: {}", stdout(output));
    }
    let publication = fixture.scan(&["publication", &fixture.base, &head], Some(GITLEAKS_STUB), &leak());
    assert_eq!(publication.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&publication.stderr).contains("SECRET DETECTED"));
}

#[test]
fn publication_runs_secrets_then_deny_list_then_email() {
    let fixture = Fixture::new();
    commit(&fixture.repo, "private author", &[("GIT_AUTHOR_EMAIL", PRIVATE)]);
    let head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let refused = fixture.scan(&["publication", &fixture.base, &head], Some(GITLEAKS_STUB), &[]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(stdout(&refused).contains("author email"), "{}", stdout(&refused));
    assert!(fixture.trace().contains(&format!("detect --no-banner --redact --log-opts={}..{head}", fixture.base)));
    let absent_base = fixture.scan(&["publication", "", &fixture.base], Some(GITLEAKS_STUB), &[]);
    assert_eq!(absent_base.status.code(), Some(0), "{}", stdout(&absent_base));
    assert!(fixture.trace().ends_with(&format!("--log-opts={}\n", fixture.base)));
    let missing = fixture.scan(&["publication", &fixture.base, &head], None, &[("PATH", "/nonexistent")]);
    assert_ne!(missing.status.code(), Some(0));
}
