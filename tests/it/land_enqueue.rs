use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_landing::land::enqueue::{Environment, Queued, enqueue, issue_numbers, pick_issue, run};
use north_landing::land::queue::{self, RedFix, Ticket};
use north_landing::landing::lane_squash::Squash;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    root: PathBuf,
    _directory: tempfile::TempDir,
    origin: PathBuf,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (origin, repo, state) = (root.join("origin.git"), root.join("repo"), root.join("state"));
        git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&root, &["init", "-q", "-b", "main", "repo"]);
        git(&repo, &["config", "user.name", "Enqueue scenario"]);
        git(&repo, &["config", "user.email", "enqueue@example.invalid"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        fs::write(repo.join("file"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "Start the repository"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["switch", "-qc", "lane"]);
        Fixture { root, _directory: directory, origin, repo, state }
    }

    fn commit(&self, repo: &Path, name: &str, message: &str) {
        fs::write(repo.join(name), format!("{name}\n")).unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", message]);
    }

    fn enqueue(&self, repo: &Path) -> Queued {
        let environment = Environment { state_home: &self.state, host: "laptop", session: Some("worker-7") };
        enqueue(repo, None, &environment).unwrap()
    }

    fn queue(&self) -> PathBuf {
        queue::directory(&self.state, &git(&self.repo, &["remote", "get-url", "--push", "origin"]))
    }

    fn pending(&self) -> Vec<Ticket> {
        queue::pending(&self.queue()).unwrap().into_iter().map(|(_, ticket)| ticket).collect()
    }
}

#[test]
fn enqueue_returns() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let main_before = git(&fixture.origin, &["rev-parse", "main"]);
    let queued = fixture.enqueue(&fixture.repo);
    assert_eq!(git(&fixture.origin, &["rev-parse", "main"]), main_before, "enqueue must not push main");
    let tickets = fixture.pending();
    assert_eq!(tickets, vec![queued.ticket.clone()]);
    let ticket = &tickets[0];
    assert_eq!(ticket.head, git(&fixture.repo, &["rev-parse", "HEAD"]));
    assert_eq!(ticket.branch, "lane");
    assert_eq!(ticket.root, fixture.repo);
    assert_eq!(ticket.host, "laptop");
    assert_eq!(ticket.session.as_deref(), Some("worker-7"));
    assert_eq!(ticket.origin, fixture.origin.to_str().unwrap());
    assert_eq!(ticket.red_fix, None);
    assert!(ticket.verdict.is_none());
    assert_eq!(ticket.queued_at.len(), 20, "{}", ticket.queued_at);
    assert!(ticket.queued_at.ends_with('Z') && ticket.queued_at.as_bytes()[10] == b'T');
    let name = queued.path.file_name().unwrap().to_str().unwrap();
    assert!(name.ends_with("-lane.json") && name.split('-').next().unwrap().parse::<u128>().is_ok(), "{name}");
    let entries: Vec<_> = fs::read_dir(fixture.queue()).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(entries.len(), 1, "no temporary file or lock may remain: {entries:?}");
    let state: Vec<_> = fs::read_dir(fixture.state.join("north")).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(state, vec!["land-queue"]);
}

#[test]
fn enqueue_squash() {
    let fixture = Fixture::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fixture.commit(&fixture.repo, name, &format!("Add {name}"));
    }
    let base = git(&fixture.repo, &["rev-parse", "origin/main"]);
    assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{base}..HEAD")]), "3");
    let queued = fixture.enqueue(&fixture.repo);
    assert!(matches!(queued.squash, Squash::Squashed { commits: 3, .. }), "{:?}", queued.squash);
    assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{base}..HEAD")]), "1");
    assert_eq!(queued.ticket.head, git(&fixture.repo, &["rev-parse", "HEAD"]));
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD^"]), base);
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(fixture.repo.join(name).is_file());
    }
    assert_eq!(fixture.pending()[0].head, queued.ticket.head);
}

#[test]
fn enqueue_red_fix_marker() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    fs::write(fixture.repo.join("fix.txt"), "fix\n").unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Repair the failing test", "-m", "Main-red: #812"]);
    let queued = fixture.enqueue(&fixture.repo);
    assert_eq!(queued.ticket.red_fix, Some(RedFix::Marker));
    assert!(git(&fixture.repo, &["log", "-1", "--format=%B"]).contains("Main-red: #812"));
    assert_eq!(fixture.pending()[0].red_fix, Some(RedFix::Marker));
    let text = fs::read_to_string(&queued.path).unwrap();
    assert!(text.contains("\"red_fix\": \"marker\""), "{text}");

    let plain = Fixture::new();
    plain.commit(&plain.repo, "a.txt", "Add a");
    git(&plain.repo, &["commit", "-q", "--allow-empty", "-m", "Mention Main-red: in prose", "-m", "see Main-red:"]);
    assert_eq!(plain.enqueue(&plain.repo).ticket.red_fix, None);
}

#[test]
fn enqueue_names_an_issue_only_when_the_open_red_issue_is_cited() {
    let listing = r#"[{"number":12,"title":"main is red"},{"number":30,"title":"main is red again"}]"#;
    assert_eq!(pick_issue(listing, "main", &[12]), Some(RedFix::Issue(12)));
    assert_eq!(pick_issue(listing, "main", &[30]), None);
    assert_eq!(pick_issue(listing, "main", &[]), None);
    assert_eq!(pick_issue("[]", "main", &[12]), None);
    assert_eq!(pick_issue("not json", "main", &[12]), None);
    assert_eq!(issue_numbers("Fix the build (#12)\n- also #7 and #x and # 9"), vec![12, 7]);
}

#[test]
fn enqueue_replaces() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let other = fixture.root.join("other");
    git(&fixture.repo, &["worktree", "add", "-q", "-b", "other", other.to_str().unwrap(), "origin/main"]);
    fixture.commit(&other, "o.txt", "Add o");
    let first = fixture.enqueue(&fixture.repo);
    let sibling = fixture.enqueue(&other);
    assert_eq!(fixture.pending().len(), 2);

    fixture.commit(&fixture.repo, "b.txt", "Add b");
    let second = fixture.enqueue(&fixture.repo);
    assert_ne!(first.path, second.path);
    assert!(!first.path.exists());
    let tickets = fixture.pending();
    assert_eq!(tickets.len(), 2);
    let lane = tickets.iter().find(|ticket| ticket.branch == "lane").unwrap();
    assert_eq!(lane.head, second.ticket.head);
    assert_ne!(lane.head, first.ticket.head);
    assert!(tickets.iter().any(|ticket| ticket.branch == "other" && ticket.head == sibling.ticket.head));
    assert!(sibling.path.exists());
}

#[test]
fn enqueue_help_names_its_usage() {
    assert_eq!(run(&["--help".to_owned()]), 0);
}

#[test]
fn enqueue_stamps_the_ticket_with_the_second_the_lane_made_its_first_commit_even_after_squashing_it() {
    let fixture = Fixture::new();
    let seconds = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let before = seconds();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    fixture.commit(&fixture.repo, "b.txt", "Add b");
    let queued = fixture.enqueue(&fixture.repo);
    assert!(matches!(queued.squash, Squash::Squashed { commits: 2, .. }), "{:?}", queued.squash);
    let first = queued.ticket.first_commit_secs.expect("the ticket carries the lane's first commit time");
    assert!(first >= before && first < seconds(), "first commit {first} is the second of the lane's first commit, not of the squash or the enqueue (started {before})");
    assert_eq!(fixture.pending()[0].first_commit_secs, Some(first));
}

fn north_enqueue(fixture: &Fixture) -> (std::process::Output, std::time::Duration) {
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["land", "enqueue", "--repo"])
        .arg(&fixture.repo)
        .env("XDG_STATE_HOME", &fixture.state)
        .env_remove("NORTH_LAND_SERVING")
        .output()
        .unwrap();
    (output, started.elapsed())
}

fn assert_refused_before_queueing(fixture: &Fixture, gate_line: &str) {
    let (output, elapsed) = north_enqueue(fixture);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains(gate_line), "{stdout}{stderr}");
    assert!(stderr.contains("NOT queued"), "{stderr}");
    assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
    assert!(!fixture.state.exists(), "a refused lane writes no ticket to the queue or the outbox: {:?}", fs::read_dir(&fixture.state).map(|entries| entries.count()));
}

#[test]
fn enqueue_refuses_a_lane_that_grows_a_script_without_an_allowlist_entry_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "tool.py", "Add a python tool");
    assert_refused_before_queueing(&fixture, "tool.py: R1 new Python path is forbidden or Script path is not allowlisted; write it in Rust under src/ or as a north subcommand");
}

#[test]
fn enqueue_refuses_a_lane_that_adds_a_dependency_without_a_reason_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("package.json"), r#"{"dependencies":{"left-pad":"1.3.0"}}"#).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add left-pad"]);
    assert_refused_before_queueing(&fixture, "dep-gate: package.json: left-pad: added dependency needs a reason");
}

#[test]
fn enqueue_queues_a_lane_whose_dependency_names_its_reason() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("package.json"), r#"{"dependencies":{"left-pad":"1.3.0"}}"#).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add left-pad", "-m", "Dependency: left-pad — pads strings"]);
    let (output, _) = north_enqueue(&fixture);
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(fixture.pending().len(), 1);
}
