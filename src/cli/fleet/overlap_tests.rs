use std::fs;
use std::path::Path;
use std::process::Command;

use north_landing::land::queue::{Ticket, file_name, write};

use super::*;

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

struct Container {
    root: tempfile::TempDir,
}

impl Container {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin.git");
        let main = root.path().join("main");
        git(root.path(), &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(root.path(), &["init", "-q", "-b", "main", "main"]);
        fs::write(main.join("x.rs"), "1").unwrap();
        fs::write(main.join("y.rs"), "1").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "base"]);
        git(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&main, &["push", "-q", "origin", "main"]);
        git(&main, &["fetch", "-q", "origin"]);
        Self { root }
    }

    fn main(&self) -> PathBuf {
        self.root.path().join("main")
    }

    fn lane(&self, slug: &str) -> PathBuf {
        let path = self.root.path().join("worktrees").join(slug);
        git(&self.main(), &["worktree", "add", "-q", "-b", slug, path.to_str().unwrap(), "origin/main"]);
        path
    }
}

#[test]
fn a_lane_changed_files_are_its_commits_its_edits_and_its_new_files_against_origin_main() {
    let container = Container::new();
    let lane = container.lane("one");
    fs::write(lane.join("x.rs"), "2").unwrap();
    git(&lane, &["commit", "-q", "-am", "x"]);
    fs::write(lane.join("y.rs"), "2").unwrap();
    fs::write(lane.join("new.rs"), "n").unwrap();
    assert_eq!(changed_files(&lane).unwrap().into_iter().collect::<Vec<_>>(), ["new.rs", "x.rs", "y.rs"]);
    assert_eq!(changed_files(&container.main()).unwrap().len(), 0);
}

#[test]
fn a_lane_that_main_moved_past_does_not_count_the_files_main_changed() {
    let container = Container::new();
    let lane = container.lane("one");
    let main = container.main();
    fs::write(main.join("y.rs"), "main moved").unwrap();
    git(&main, &["commit", "-q", "-am", "main"]);
    git(&main, &["push", "-q", "origin", "main"]);
    git(&lane, &["fetch", "-q", "origin"]);
    fs::write(lane.join("x.rs"), "2").unwrap();
    assert_eq!(changed_files(&lane).unwrap().into_iter().collect::<Vec<_>>(), ["x.rs"]);
}

#[test]
fn the_worktrees_and_the_common_directory_of_a_lane_and_of_main_agree() {
    let container = Container::new();
    let lane = container.lane("one");
    let (main, from_lane) = (Repo::of(&container.main()).unwrap(), Repo::of(&lane).unwrap());
    assert_eq!(main.common, from_lane.common);
    assert_eq!(main.worktrees().unwrap(), fs::canonicalize(container.root.path()).unwrap().join("worktrees"));
    assert!(Repo::of(Path::new("/nonexistent-staffing-overlap")).is_none());
}

#[test]
fn a_ticket_queued_at_the_lander_is_a_lane_with_the_task_of_the_session_that_queued_it() {
    let container = Container::new();
    let lane = container.lane("queued-lane");
    fs::write(lane.join("x.rs"), "2").unwrap();
    git(&lane, &["commit", "-q", "-am", "x"]);
    let head = git(&lane, &["rev-parse", "HEAD"]).trim().to_owned();
    let repo = Repo::of(&container.main()).unwrap();
    let origin = git(&container.main(), &["remote", "get-url", "--push", "origin"]);
    let state = tempfile::tempdir().unwrap();
    let directory = north_landing::land::queue::directory(state.path(), origin.trim());
    let ticket = Ticket { origin: origin.trim().into(), root: lane, host: "h".into(), branch: "queued-lane".into(), head, session: Some("worker".into()), queued_at: "t".into(), ..Default::default() };
    write(&directory.join(file_name(5_000_000, "queued-lane")), &ticket).unwrap();
    let tasks = [("worker".to_owned(), Some("north#7".to_owned()))];
    let lanes = ticket_lanes(&repo, &tasks, state.path());
    assert_eq!(lanes.len(), 1);
    assert_eq!((lanes[0].name.as_str(), lanes[0].task.as_deref(), lanes[0].since_ms), ("queued-lane", Some("north#7"), 5000));
    assert_eq!(lanes[0].files.iter().collect::<Vec<_>>(), ["x.rs"]);
    assert!(ticket_lanes(&repo, &tasks, &state.path().join("elsewhere")).is_empty());
}

#[test]
fn a_launch_that_overlaps_is_judged_by_the_files_line_and_the_regenerate_drivers_of_origin_main() {
    let container = Container::new();
    let lane = container.lane("one");
    fs::write(lane.join("x.rs"), "2").unwrap();
    let lanes = [Lane { name: "one".into(), task: Some("north#1".into()), since_ms: 1, files: changed_files(&lane).unwrap() }];
    let declared = declared_files("Item: north#2 y\nFiles: x.rs\n");
    assert!(matches!(assess(&declared, &lanes, &|_| false, Policy::default()), Verdict::Warn(_)));
    fs::write(container.main().join(".gitattributes"), "x.rs merge=set\n").unwrap();
    git(&container.main(), &["add", ".gitattributes"]);
    git(&container.main(), &["commit", "-q", "-m", "attributes"]);
    git(&container.main(), &["push", "-q", "origin", "main"]);
    git(&container.main(), &["fetch", "-q", "origin"]);
    let driven = MergeDriven::parse(&Repo::of(&container.main()).unwrap().attributes().unwrap());
    assert_eq!(assess(&declared, &lanes, &|file| driven.covers(file), Policy::default()), Verdict::Clear);
}

#[test]
fn the_excuse_is_recorded_beside_the_placement_note() {
    assert_eq!(noted(None, None), None);
    assert_eq!(noted(Some("pin: x".into()), None).as_deref(), Some("pin: x"));
    assert_eq!(noted(None, Some("lanes edit different hunks")).as_deref(), Some("overlap-ok: lanes edit different hunks"));
    assert_eq!(noted(Some("pin: x".into()), Some("why")).as_deref(), Some("pin: x; overlap-ok: why"));
}

#[test]
fn a_queued_box_is_added_with_the_followed_task_tier_and_parent_of_the_launch() {
    assert_eq!(queue_words("north#9", "sonnet-high", "lead"), ["threads", "queue", "follow", "north#9", "--tier", "sonnet-high", "--parent", "lead"]);
}

#[test]
fn a_lane_holding_the_launchs_own_task_is_not_an_overlap_however_the_task_is_spelled() {
    let lane = |name: &str, task: Option<&str>| Lane { name: name.into(), task: task.map(str::to_owned), since_ms: 1, files: BTreeSet::new() };
    let lanes = vec![lane("same", Some("tompassarelli/north#9")), lane("other", Some("north#10")), lane("untasked", None)];
    let kept = others(lanes, &["north#9".to_owned()]);
    assert_eq!(kept.iter().map(|lane| lane.name.as_str()).collect::<Vec<_>>(), ["other", "untasked"]);
}
