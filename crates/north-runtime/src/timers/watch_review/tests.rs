use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::fleet_session::{Launch, LaunchError, Placement, held, placement};
use super::system::System;
use super::{Moved, Source, World, brief, due, execute, review, sources};

const TODAY: &str = "2026-11-04";
const STAMP: &str = "202611040900";
const TIER_REASON: &str = "followed-source reviews decide what enters the harness; Tom chose xhigh for harness-level work (2026-11-04)";
const EXHAUSTED: &str = "claude:a unavailable: exhausted: provider week 100%; no Claude account is available; wait for a window to reset";

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(repo: &Path, file: &str, count: usize) {
    for index in 0..count {
        fs::create_dir_all(repo.join(file).parent().unwrap()).unwrap();
        fs::write(repo.join(file), format!("{file} {index}\n")).unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", &format!("change {index}")]);
    }
}

fn upstream(root: &Path, name: &str, inside: &str, inside_commits: usize, outside_commits: usize) -> (PathBuf, String) {
    let repo = root.join(name);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    commit(&repo, "root/readme", 1);
    let base = git(&repo, &["rev-parse", "--short", "HEAD"]);
    commit(&repo, inside, inside_commits);
    commit(&repo, "elsewhere/file", outside_commits);
    (repo, base)
}

#[test]
fn watch_review_dry_run_names_exactly_the_sources_that_are_due() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let cases = [
        ("pstack", "pstack", "pstack/skill", 10, 1, "2026-10-27"),
        ("mattpocock-skills", "", "skills/note", 3, 0, "2026-07-10"),
        ("reviewed-today", "", "skills/note", 0, 0, TODAY),
        ("quiet-recent", "", "skills/note", 3, 0, "2026-11-01"),
        ("stale-unmoved", "", "skills/note", 0, 0, "2026-07-10"),
        ("outside-path", "pstack", "other/file", 4, 0, "2026-07-10"),
    ];
    let mut toml = String::new();
    let mut expected = Vec::new();
    for (id, path, inside, inside_commits, outside_commits, reviewed) in cases {
        let (repo, base) = upstream(root, id, inside, inside_commits, outside_commits);
        let head = git(&repo, &["rev-parse", "--short", "HEAD"]);
        toml.push_str(&format!(
            "[[source]]\nid = \"{id}\"\nperson = \"x\"\nrepo = \"{}\"\npath = \"{path}\"\nlicense = \"MIT\"\nlast_reviewed = {reviewed}\nlast_commit = \"{base}\"\ntook = []\n\n",
            repo.display()
        ));
        if id == "pstack" {
            expected.push(format!("watch-review: launch {id} (10 commits, {base}..{head})"));
        }
        if id == "mattpocock-skills" {
            expected.push(format!("watch-review: launch {id} (3 commits, {base}..{head})"));
        }
    }
    let system = System::at(root.join("watch.toml"), root.join("cache"));
    let done = review(&system, sources(&toml).unwrap(), TODAY, STAMP, true);
    assert_eq!(done.lines, expected);
    assert!(done.held.is_empty() && done.failed.is_empty(), "{done:?}");
}

#[test]
fn watch_review_due_rule_reads_age_and_commit_counts() {
    let source = |reviewed| Source { id: "s".into(), repo: "r".into(), path: String::new(), last_reviewed_day: reviewed, last_commit: "abcdef0".into() };
    let moved = |count| Moved { count, head: "h".into() };
    assert!(due(&source(100), &moved(1), 130));
    assert!(!due(&source(100), &moved(1), 129));
    assert!(!due(&source(100), &moved(0), 400));
    assert!(due(&source(129), &moved(10), 130));
    assert!(!due(&source(129), &moved(9), 130));
}

struct Fake {
    moved: BTreeMap<String, Result<Moved, String>>,
    place: Result<Placement, String>,
    launch: Result<(), LaunchError>,
    sessions: Vec<String>,
    worktrees: RefCell<Vec<String>>,
    discarded: RefCell<Vec<String>>,
    launched: RefCell<Vec<(String, String, PathBuf, String)>>,
    sent: RefCell<Vec<String>>,
}

impl Fake {
    fn healthy(ids: &[&str]) -> Self {
        Self {
            moved: ids.iter().map(|id| ((*id).to_owned(), Ok(Moved { count: 12, head: "beef123".into() }))).collect(),
            place: Ok(Placement::Account("b".into())),
            launch: Ok(()),
            sessions: Vec::new(),
            worktrees: RefCell::default(),
            discarded: RefCell::default(),
            launched: RefCell::default(),
            sent: RefCell::default(),
        }
    }
}

impl World for Fake {
    fn moved(&self, source: &Source) -> Result<Moved, String> {
        self.moved[&source.id].clone()
    }
    fn sessions(&self) -> Result<Vec<String>, String> {
        Ok(self.sessions.clone())
    }
    fn place(&self, _brief: &str) -> Result<Placement, String> {
        self.place.clone()
    }
    fn worktree(&self, name: &str) -> Result<PathBuf, String> {
        self.worktrees.borrow_mut().push(name.to_owned());
        Ok(PathBuf::from("/lanes").join(name))
    }
    fn discard(&self, name: &str, _path: &Path) {
        self.discarded.borrow_mut().push(name.to_owned());
    }
    fn launch(&self, launch: &Launch) -> Result<(), LaunchError> {
        self.launched.borrow_mut().push((launch.name.to_owned(), launch.account.to_owned(), launch.cwd.to_path_buf(), launch.brief.to_owned()));
        self.launch.clone()
    }
    fn tell_proxy(&self, text: &str) -> Result<(), String> {
        self.sent.borrow_mut().push(text.to_owned());
        Ok(())
    }
}

fn list(ids: &[&str]) -> String {
    ids.iter()
        .map(|id| format!("[[source]]\nid = \"{id}\"\nrepo = \"https://example.invalid/{id}\"\npath = \"\"\nlast_reviewed = 2026-07-10\nlast_commit = \"abcdef0\"\n\n"))
        .collect()
}

#[test]
fn watch_review_launch_held_for_capacity_leaves_the_source_due_and_posts_one_message() {
    let ids = ["pstack", "mattpocock-skills"];
    let text = list(&ids);

    let mut held_by_placement = Fake::healthy(&ids);
    held_by_placement.place = Ok(Placement::Held(EXHAUSTED.into()));
    assert_eq!(execute(&held_by_placement, &text, TODAY, STAMP, false), 0);
    assert!(held_by_placement.launched.borrow().is_empty() && held_by_placement.worktrees.borrow().is_empty());
    let sent = held_by_placement.sent.borrow();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("pstack") && sent[0].contains("mattpocock-skills") && sent[0].contains("stays due"), "{sent:?}");

    let mut refused = Fake::healthy(&ids);
    refused.launch = Err(LaunchError::Held(EXHAUSTED.into()));
    assert_eq!(execute(&refused, &text, TODAY, STAMP, false), 0);
    assert_eq!(refused.discarded.borrow().len(), 2, "a refused launch leaves no lane behind");
    assert_eq!(refused.sent.borrow().len(), 1);

    let next_day = Fake::healthy(&ids);
    assert_eq!(execute(&next_day, &text, TODAY, STAMP, false), 0);
    assert_eq!(next_day.launched.borrow().len(), 2, "the next run launches the sources again from the same list");
    assert!(next_day.sent.borrow().is_empty());
}

#[test]
fn watch_review_other_failures_exit_nonzero_with_one_message() {
    let text = list(&["a", "b", "c"]);
    let mut world = Fake::healthy(&["a", "b", "c"]);
    world.moved.insert("a".into(), Err("fetch failed: no route".into()));
    world.moved.insert("b".into(), Err("abcdef0 not found".into()));
    world.launch = Err(LaunchError::Failed("north fleet launch exited 1: brief refused".into()));
    assert_eq!(execute(&world, &text, TODAY, STAMP, false), 1);
    let sent = world.sent.borrow();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("a: fetch failed") && sent[0].contains("b: abcdef0 not found") && sent[0].contains("c: north fleet launch exited 1"), "{sent:?}");
}

#[test]
fn watch_review_skips_live_sessions_and_sends_nothing_when_quiet() {
    let text = list(&["pstack"]);
    let mut world = Fake::healthy(&["pstack"]);
    world.sessions = vec!["watch-review-pstack-202611030900\tclaude\tb\trunning".into()];
    assert_eq!(execute(&world, &text, TODAY, STAMP, false), 0);
    assert!(world.launched.borrow().is_empty() && world.sent.borrow().is_empty());

    world.sessions = vec!["watch-review-pstack-202611030900\tclaude\tb\texited".into()];
    assert_eq!(execute(&world, &text, TODAY, STAMP, false), 0);
    assert_eq!(world.launched.borrow().len(), 1);
}

#[test]
fn watch_review_launches_the_placed_account_in_a_fresh_lane_with_the_xhigh_brief() {
    let world = Fake::healthy(&["pstack"]);
    assert_eq!(execute(&world, &list(&["pstack"]), TODAY, STAMP, false), 0);
    let launched = world.launched.borrow();
    let (name, account, cwd, text) = &launched[0];
    assert_eq!((name.as_str(), account.as_str()), ("watch-review-pstack-202611040900", "b"));
    assert_eq!(cwd, &PathBuf::from("/lanes/watch-review-pstack-202611040900"));
    let header: Vec<&str> = text.lines().take_while(|line| !line.trim().is_empty()).collect();
    let reason = format!("Tier-reason: {TIER_REASON}");
    for line in ["Category: research", "Tier: worker-xhigh", reason.as_str(), "Spec: judged", "Scope: cross-repo", "Surface: docs", "Verify: none"] {
        assert!(header.contains(&line), "{line} missing from {header:?}");
    }
    assert!(header.iter().any(|line| line.starts_with("Done when:") && line.contains("docs/rationale/watch/2026-11-04-pstack.md") && line.contains("agents/watch.toml")));
    assert!(header.iter().any(|line| line.starts_with("ETA:")));
    let head = header.join("\n");
    assert_eq!(crate::dispatch::tier_reason(&head, None).as_deref(), Some(TIER_REASON));
    assert_eq!(crate::dispatch::untasked(&head, None, None), Ok(true));
}

#[test]
fn watch_review_brief_header_stays_before_the_first_blank_line() {
    let source = Source { id: "x".into(), repo: "https://example.invalid/x".into(), path: "sub".into(), last_reviewed_day: 0, last_commit: "abcdef0".into() };
    let text = brief(&source, &Moved { count: 11, head: "1234567".into() }, TODAY);
    assert_eq!(text.lines().take_while(|line| !line.trim().is_empty()).count(), 11);
}

#[test]
fn watch_review_reads_placement_and_launch_refusals() {
    assert_eq!(placement("test.: target gex45 claude:d (code-only box on public north)\n"), Some(Placement::Account("d".into())));
    assert_eq!(placement("noise\nx: target held: no Claude account is available\n"), Some(Placement::Held("held: no Claude account is available".into())));
    assert!(matches!(placement("x: target anthropic-cloud (credit; rust)"), Some(Placement::Held(_))));
    assert_eq!(placement("nothing here"), None);
    assert!(held(EXHAUSTED));
    assert!(held("gex45 skipped: 14.0 GiB free on the root disk, under 15; still saturated after waiting 2 min"));
    assert!(!held("north fleet launch exited 2: brief refused: Category bug has no Class: line"));
}

#[test]
fn watch_review_rejects_sources_with_missing_or_unsafe_fields() {
    let text = "[[source]]\nid = \"ok\"\nrepo = \"r\"\nlast_reviewed = 2026-07-10\nlast_commit = \"abcdef0\"\n\n[[source]]\nid = \"bad\"\nrepo = \"-x\"\nlast_reviewed = 2026-07-10\nlast_commit = \"abcdef0\"\n\n[[source]]\nid = \"nodate\"\nrepo = \"r\"\nlast_commit = \"abcdef0\"\n";
    let found = sources(text).unwrap();
    assert!(found[0].is_ok());
    assert_eq!(found[1].as_ref().unwrap_err(), "bad: invalid id, repo or last_commit");
    assert_eq!(found[2].as_ref().unwrap_err(), "nodate: last_reviewed is required");
}
