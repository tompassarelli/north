use std::path::Path;
use std::process::Command;

use super::*;

const PUSHER: &str = "safe-push-test@example.invalid";
const PRIVATE: &str = "someone@private.example";

fn git(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_EMAIL")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}

fn empty_commit(dir: &Path, message: &str, env: &[(&str, &str)]) {
    git(dir, &["commit", "--allow-empty", "-qm", message], env);
}

struct Repository {
    _scratch: tempfile::TempDir,
    dir: std::path::PathBuf,
    base: String,
}

fn repository() -> Repository {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"], &[]);
    git(&dir, &["config", "user.name", "safe-push-test"], &[]);
    git(&dir, &["config", "user.email", PUSHER], &[]);
    std::fs::write(dir.join("content.txt"), "initial\n").unwrap();
    git(&dir, &["add", "content.txt"], &[]);
    git(&dir, &["commit", "-qm", "initial"], &[]);
    let base = git(&dir, &["rev-parse", "HEAD"], &[]);
    Repository { _scratch: scratch, dir, base }
}

fn rust_scan(repository: &Repository, pushing: &[(&str, &str)]) -> (i32, String) {
    let range = format!("{}..HEAD", repository.base);
    let emails: Vec<String> = pushing.iter().map(|(_, email)| (*email).to_owned()).collect();
    match outgoing_email_refusal(&repository.dir, &[range], &emails) {
        Some(message) => (1, format!("{message}\n")),
        None => (0, String::new()),
    }
}

fn same_verdict(_case: &str, repository: &Repository, pushing: &[(&str, &str)]) -> (i32, String) {
    rust_scan(repository, pushing)
}

fn refused(case: &str, repository: &Repository, pushing: &[(&str, &str)], role: &str, commit: &str) {
    let (status, stdout) = same_verdict(case, repository, pushing);
    let short = git(&repository.dir, &["rev-parse", "--short", commit], &[]);
    assert_eq!(status, 1, "{case}: {stdout}");
    assert!(stdout.starts_with(&format!("safe-push: commit {short} {role} email {PRIVATE} is not {PUSHER}")), "{case}: {stdout}");
}

#[test]
fn safe_push_email_scan_refuses_private_authors_and_committers() {
    let author = repository();
    empty_commit(&author.dir, "private author", &[("GIT_AUTHOR_EMAIL", PRIVATE)]);
    empty_commit(&author.dir, "later clean commit", &[]);
    refused("author", &author, &[], "author", "HEAD~1");

    let committer = repository();
    empty_commit(&committer.dir, "private committer", &[("GIT_COMMITTER_EMAIL", PRIVATE)]);
    refused("committer", &committer, &[], "committer", "HEAD");

    let applied = repository();
    std::fs::write(applied.dir.join("content.txt"), "cloud work\n").unwrap();
    git(&applied.dir, &["commit", "-qam", "cloud patch"], &[("GIT_AUTHOR_NAME", "someone"), ("GIT_AUTHOR_EMAIL", PRIVATE)]);
    let patches = applied.dir.with_file_name("patches");
    git(&applied.dir, &["format-patch", "-q", "-1", "-o", &patches.to_string_lossy()], &[]);
    git(&applied.dir, &["reset", "-q", "--hard", "HEAD~1"], &[]);
    let patch = std::fs::read_dir(&patches).unwrap().next().unwrap().unwrap().path();
    git(&applied.dir, &["am", "-q", &patch.to_string_lossy()], &[]);
    refused("git am", &applied, &[], "author", "HEAD");

    let other_identity = repository();
    empty_commit(&other_identity.dir, "env identity", &[("GIT_AUTHOR_EMAIL", PRIVATE), ("GIT_COMMITTER_EMAIL", PRIVATE)]);
    refused("another pusher's identity", &other_identity, &[], "author", "HEAD");

    let unset = repository();
    git(&unset.dir, &["config", "user.email", ""], &[]);
    let (status, stdout) = same_verdict("unset user.email", &unset, &[]);
    assert_eq!(status, 1);
    assert!(stdout.starts_with("safe-push: ✗ git config user.email is empty"), "{stdout}");
}

#[test]
fn safe_push_email_scan_passes_the_pusher_and_allowed_emails() {
    let allowed = repository();
    git(&allowed.dir, &["config", "--add", "safe-push.allowEmail", "bot@allowed.example"], &[]);
    git(&allowed.dir, &["config", "--add", "safe-push.allowEmail", "Release@Allowed.Example"], &[]);
    empty_commit(&allowed.dir, "pusher email in another case", &[("GIT_AUTHOR_EMAIL", "Safe-Push-Test@Example.Invalid")]);
    empty_commit(&allowed.dir, "allowed bot", &[("GIT_AUTHOR_EMAIL", "bot@allowed.example"), ("GIT_COMMITTER_EMAIL", "release@allowed.example")]);
    git(&allowed.dir, &["commit", "--allow-empty", "-qm", "co-authored", "-m", &format!("Co-Authored-By: someone <{PRIVATE}>")], &[]);
    assert_eq!(same_verdict("allowEmail", &allowed, &[]), (0, String::new()));

    let identity = [("GIT_AUTHOR_EMAIL", "ci@env.example"), ("GIT_COMMITTER_EMAIL", "ci@env.example")];
    let pushing = repository();
    empty_commit(&pushing.dir, "env identity", &identity);
    assert_eq!(same_verdict("the pushing identity", &pushing, &identity), (0, String::new()));

    let nothing = repository();
    assert_eq!(same_verdict("no outgoing commits", &nothing, &[]), (0, String::new()));
}

#[derive(Clone, Copy)]
enum Listed {
    Absent,
    Missing,
    Present,
}

#[derive(Clone, Copy)]
enum Scanned {
    Clean,
    Found(&'static [&'static str]),
    Failed(&'static str),
}

struct DenylistRow {
    case: &'static str,
    listed: Listed,
    south: bool,
    visibility: Option<&'static str>,
    scanned: Scanned,
    refused: bool,
    says: &'static str,
}

fn rust_denylist(row: &DenylistRow, config: &Path, south: &Path) -> (i32, String) {
    let denylist = Denylist { south: south.to_path_buf(), config: (!config.as_os_str().is_empty()).then(|| config.to_path_buf()) };
    let verdict = denylist.verdict(
        || row.visibility.unwrap_or("UNKNOWN").to_owned(),
        || match row.scanned {
            Scanned::Clean => Ok(BTreeSet::new()),
            Scanned::Found(found) => Ok(found.iter().map(|finding| (*finding).to_owned()).collect()),
            Scanned::Failed(error) => Err(error.to_owned()),
        },
    );
    (i32::from(verdict.refused), verdict.lines.iter().map(|line| format!("{line}\n")).collect())
}

const FOUND: Scanned = Scanned::Found(&["fake-private-term in identity.txt"]);

const DENYLIST_ROWS: [DenylistRow; 9] = [
    DenylistRow { case: "no list and no south", listed: Listed::Absent, south: false, visibility: Some("PUBLIC"), scanned: FOUND, refused: false, says: "so private values were not scanned" },
    DenylistRow { case: "list missing beside south", listed: Listed::Missing, south: true, visibility: Some("PUBLIC"), scanned: Scanned::Clean, refused: true, says: "is missing although" },
    DenylistRow { case: "private repository", listed: Listed::Present, south: true, visibility: Some("PRIVATE"), scanned: FOUND, refused: false, says: "" },
    DenylistRow { case: "internal repository", listed: Listed::Present, south: false, visibility: Some("INTERNAL"), scanned: FOUND, refused: false, says: "" },
    DenylistRow { case: "clean public push", listed: Listed::Present, south: false, visibility: Some("PUBLIC"), scanned: Scanned::Clean, refused: false, says: "" },
    DenylistRow { case: "public findings", listed: Listed::Present, south: true, visibility: Some("PUBLIC"), scanned: Scanned::Found(&["fake-private-term in identity.txt", "fake-skill-host in harness/skills/owner-notes.md"]), refused: true, says: "denylist: fake-skill-host in harness/skills/owner-notes.md\nsafe-push: ✗ PRIVATE VALUE for a PUBLIC repository matched" },
    DenylistRow { case: "unknown visibility", listed: Listed::Present, south: false, visibility: None, scanned: Scanned::Found(&["fake-private-term in commit 1234567"]), refused: true, says: "PRIVATE VALUE for a UNKNOWN repository" },
    DenylistRow { case: "scan failure", listed: Listed::Present, south: false, visibility: Some("PUBLIC"), scanned: Scanned::Failed("the deny-list is unreadable"), refused: true, says: "denylist: cannot scan: the deny-list is unreadable\nsafe-push: ✗ the deny-list scan could not run (exit 2)" },
    DenylistRow { case: "list missing without south", listed: Listed::Missing, south: false, visibility: Some("PUBLIC"), scanned: Scanned::Clean, refused: false, says: "so private values were not scanned" },
];

#[test]
fn safe_push_denylist_stage_decides() {
    for row in &DENYLIST_ROWS {
        let scratch = tempfile::tempdir().unwrap();
        let config = scratch.path().join("denylist.toml");
        let south = scratch.path().join("south");
        if matches!(row.listed, Listed::Present) {
            std::fs::write(&config, "title = \"rows\"\n").unwrap();
        }
        if row.south {
            std::fs::create_dir_all(&south).unwrap();
        }
        let config = if matches!(row.listed, Listed::Absent) { PathBuf::new() } else { config };
        let (status, text) = rust_denylist(row, &config, &south);
        assert_eq!(status, i32::from(row.refused), "{}: {text}", row.case);
        assert!(text.contains(row.says), "{}: {text}", row.case);
        assert_eq!(text.is_empty(), row.says.is_empty(), "{}: {text}", row.case);
        assert!(!text.contains("zebra-quokka-7731"), "{}: printed a matched value", row.case);
    }
}
