use std::fs;
use std::path::Path;
use std::process::Command;

use north_landing::land::candidate::{self, BRANCH};
use north_landing::land::queue::Ticket;

const NORTH: &str = env!("CARGO_BIN_EXE_north");

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).env("GIT_EDITOR", "true").output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn note(repo: &Path, name: &str, title: &str) {
    fs::write(repo.join("docs/rationale").join(name), format!("# {title}\n\nWhy {title} exists. More detail.\n")).unwrap();
}

#[test]
fn two_lanes_that_each_add_a_rationale_note_on_a_main_without_the_index_land_a_candidate_listing_both() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("docs/rationale")).unwrap();
    fs::create_dir_all(repo.join("harness")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]);
    }
    let sets = format!("[[set]]\npath = \"docs/rationale/README.md\"\nkind = \"regenerate\"\ncommand = \"{NORTH} docs rationale-index\"\nsources = [\"docs/rationale/\"]\n");
    fs::write(repo.join("harness/merge-sets.toml"), sets).unwrap();
    fs::write(repo.join(".gitattributes"), "docs/rationale/README.md merge=set\n").unwrap();
    note(&repo, "alpha.md", "Alpha");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let mut tickets = Vec::new();
    for (name, title) in [("beta.md", "Beta"), ("gamma.md", "Gamma")] {
        git(&repo, &["checkout", "-q", "-b", title, &base]);
        note(&repo, name, title);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", title]);
        let head = git(&repo, &["rev-parse", "HEAD"]);
        let ticket = Ticket { origin: String::new(), root: repo.clone(), host: String::new(), branch: title.to_owned(), head, session: None, queued_at: String::new(), first_commit_secs: None, several_commits: None, red_fix: None, verdict: None, keep_lane: false };
        tickets.push((dir.path().join(format!("{title}.json")), ticket));
    }
    let worktree = dir.path().join("lander");
    candidate::ensure(&repo, &worktree).unwrap();
    let built = candidate::build(&worktree, BRANCH, &base, &tickets, 2, &dir.path().join("logs")).unwrap();
    assert_eq!(built.included(), 2);
    let readme = git(&worktree, &["show", "HEAD:docs/rationale/README.md"]);
    for line in ["- [Alpha](alpha.md): Why Alpha exists.", "- [Beta](beta.md): Why Beta exists.", "- [Gamma](gamma.md): Why Gamma exists."] {
        assert!(readme.contains(line), "{readme}");
    }
    assert_eq!(git(&worktree, &["status", "--porcelain"]), "");
    let check = Command::new(NORTH).current_dir(&worktree).args(["docs", "rationale-index", "--check"]).output().unwrap();
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    assert_eq!(git(&worktree, &["rev-list", "--count", &format!("{base}..HEAD")]), "2");
}
