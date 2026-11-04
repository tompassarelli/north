use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_landing::land::enqueue::{Environment, Queued, Shape, enqueue, issue_numbers, pick_issue};
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
        git(repo, &["config", "landing.lander-host", "laptop"]);
        let south = self.root.join("no-south");
        let environment = Environment { state_home: &self.state, host: "laptop", session: Some("worker-7"), south: &south, denylist: None };
        let queued = enqueue(repo, Shape::OneCommit { message: None }, false, &environment).unwrap();
        git(repo, &["config", "--unset", "landing.lander-host"]);
        queued
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
fn enqueue_several_commits_keeps_the_lane_commits_and_carries_the_reason() {
    let fixture = Fixture::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fixture.commit(&fixture.repo, name, &format!("Add {name}"));
    }
    let head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let main_before = git(&fixture.origin, &["rev-parse", "main"]);
    let output = north_enqueue_arguments(&fixture, &["--several-commits", "each commit reverts on its own"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("lane not squashed: --several-commits: each commit reverts on its own"), "{stdout}");
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD"]), head, "the lane keeps its commits");
    assert_eq!(git(&fixture.origin, &["rev-parse", "main"]), main_before, "enqueue must not push main");
    let tickets = fixture.pending();
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].head, head);
    assert_eq!(tickets[0].several_commits.as_deref(), Some("each commit reverts on its own"));
}

#[test]
fn enqueue_refuses_several_commits_with_a_message_or_without_a_reason() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let message = fixture.root.join("message");
    fs::write(&message, "One subject\n").unwrap();
    for arguments in [vec!["--message", message.to_str().unwrap(), "--several-commits", "reason"], vec!["--several-commits", " "], vec!["--several-commits"]] {
        let output = north_enqueue_arguments(&fixture, &arguments);
        assert_eq!(output.status.code(), Some(64), "{arguments:?}: {}", String::from_utf8_lossy(&output.stdout));
    }
    assert!(fixture.pending().is_empty());
}

#[test]
fn enqueue_one_commit_tickets_carry_no_several_commits_field() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let queued = fixture.enqueue(&fixture.repo);
    assert_eq!(queued.ticket.several_commits, None);
    assert!(!fs::read_to_string(&queued.path).unwrap().contains("several_commits"));
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
fn enqueue_records_keep_lane_on_the_ticket() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let south = fixture.root.join("no-south");
    let environment = Environment { state_home: &fixture.state, host: "laptop", session: None, south: &south, denylist: None };
    let queued = enqueue(&fixture.repo, Shape::OneCommit { message: None }, true, &environment).unwrap();
    assert!(queued.ticket.keep_lane);
    assert!(fixture.pending()[0].keep_lane);
}

#[test]
fn enqueue_stamps_the_ticket_with_the_second_the_lane_made_its_first_commit_even_after_squashing_it() {
    let fixture = Fixture::new();
    let began = 1_793_000_000;
    fs::write(fixture.repo.join("a.txt"), "a.txt\n").unwrap();
    git(&fixture.repo, &["add", "."]);
    let planted = Command::new("git").current_dir(&fixture.repo).args(["commit", "-qm", "Add a"]).env("GIT_COMMITTER_DATE", format!("@{began} +0000")).status().unwrap();
    assert!(planted.success());
    fixture.commit(&fixture.repo, "b.txt", "Add b");
    let queued = fixture.enqueue(&fixture.repo);
    assert!(matches!(queued.squash, Squash::Squashed { commits: 2, .. }), "{:?}", queued.squash);
    let first = queued.ticket.first_commit_secs.expect("the ticket carries the lane's first commit time");
    assert_eq!(first, began, "the ticket carries the second of the lane's first commit, not of the squash or the enqueue");
    assert_eq!(fixture.pending()[0].first_commit_secs, Some(first));
}

fn north_enqueue(fixture: &Fixture) -> (std::process::Output, std::time::Duration) {
    north_enqueue_with(fixture, &[])
}

fn north_enqueue_with(fixture: &Fixture, environment: &[(&str, &Path)]) -> (std::process::Output, std::time::Duration) {
    north_enqueue_run(fixture, &[], environment)
}

fn north_enqueue_arguments(fixture: &Fixture, arguments: &[&str]) -> std::process::Output {
    north_enqueue_run(fixture, arguments, &[]).0
}

fn north_enqueue_run(fixture: &Fixture, arguments: &[&str], environment: &[(&str, &Path)]) -> (std::process::Output, std::time::Duration) {
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["land", "enqueue", "--repo"])
        .arg(&fixture.repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(arguments)
        .env("XDG_STATE_HOME", &fixture.state)
        .env("NORTH_LAND_NORTH", env!("CARGO_BIN_EXE_north"))
        .env("SAFE_PUSH_SOUTH", fixture.root.join("no-south"))
        .env_remove("SAFE_PUSH_DENYLIST")
        .envs(environment.iter().copied())
        .env_remove("NORTH_LAND_SERVING")
        .env_remove("SAFE_PUSH_LAND_CAPACITY")
        .env_remove("SAFE_PUSH_LAND_OFFLOAD_HOST")
        .env_remove("SAFE_PUSH_LAND_RUNNER")
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

const OFFLOAD_STUB: &str = "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"$STUB_TRACE\"\ncase \"$1 $2 $3\" in\n  'capacity offload run') exit 0 ;;\nesac\n[ \"$1\" = land-cache ] && exit 1\nexec \"$REAL_NORTH\" \"$@\"\n";

const LEASE_PROBE: &str = "printf 'acquire\\n' >>\"$LEASE_LOG\"\nstatus=0\n\"$@\" || status=$?\nprintf 'release\\n' >>\"$LEASE_LOG\"\nexit \"$status\"\n";

impl Fixture {
    fn add_settings(&self, settings: &[(&str, &str)], message: &str) {
        for (key, value) in settings {
            git(&self.repo, &["config", "--file", ".safe-push", "--add", key, value]);
        }
        git(&self.repo, &["add", ".safe-push"]);
        git(&self.repo, &["commit", "-qm", message]);
    }

    fn output_of(output: &std::process::Output) -> String {
        format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    }
}

#[test]
fn enqueue_refuses_a_lane_whose_added_land_check_exits_75_with_its_output_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fixture.add_settings(&[("land.check", "** :: printf 'the added check is broken\\n'; exit 75")], "Add a broken land check");
    let (output, _) = north_enqueue(&fixture);
    let text = Fixture::output_of(&output);
    assert!(!output.status.success(), "{text}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("the added check is broken"), "{text}");
    assert!(text.contains("exited 75") && text.contains("NOT queued"), "{text}");
    assert!(!fixture.queue().exists(), "a refused lane writes no ticket: {text}");
}

#[test]
fn enqueue_queues_a_lane_whose_added_land_check_passes() {
    let fixture = Fixture::new();
    let marker = fixture.root.join("ran");
    fixture.add_settings(&[("land.check", "** :: printf ran >\"$ENQUEUE_MARKER\"")], "Add a passing land check");
    let (output, _) = north_enqueue_with(&fixture, &[("ENQUEUE_MARKER", &marker)]);
    assert!(output.status.success(), "{}", Fixture::output_of(&output));
    assert!(marker.is_file(), "the added check ran before queueing");
    assert_eq!(fixture.pending().len(), 1);
}

#[test]
fn enqueue_runs_no_check_for_a_lane_that_adds_no_land_check_line() {
    let fixture = Fixture::new();
    let marker = fixture.root.join("ran");
    fixture.add_settings(&[("land.check", "** :: printf ran >\"$ENQUEUE_MARKER\"")], "Add a land check to main");
    git(&fixture.repo, &["push", "-q", "origin", "HEAD:main"]);
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let (output, _) = north_enqueue_with(&fixture, &[("ENQUEUE_MARKER", &marker)]);
    assert!(output.status.success(), "{}", Fixture::output_of(&output));
    assert!(!marker.exists(), "a check main already has does not run at enqueue");
    assert_eq!(fixture.pending().len(), 1);
}

#[test]
fn enqueue_offloads_an_added_land_check_to_the_configured_build_host() {
    let fixture = Fixture::new();
    let (stub, trace) = (fixture.root.join("north-stub"), fixture.root.join("trace"));
    north_core::foundation::shim::write_executable(&stub, OFFLOAD_STUB);
    fixture.add_settings(&[("land.runner", "builder-1"), ("land.check", "** :: exit 75")], "Add an offloaded land check");
    let (output, _) = north_enqueue_with(&fixture, &[("NORTH_LAND_NORTH", &stub), ("STUB_TRACE", &trace), ("REAL_NORTH", Path::new(env!("CARGO_BIN_EXE_north")))]);
    let text = Fixture::output_of(&output);
    assert!(output.status.success(), "{text}");
    let trace = fs::read_to_string(&trace).unwrap();
    assert!(trace.lines().any(|line| line.starts_with("capacity offload run --host builder-1 ") && line.ends_with("-- exit 75")), "{trace}");
    assert_eq!(fixture.pending().len(), 1);
}

#[test]
fn enqueue_runs_an_added_land_check_locally_under_one_capacity_lease() {
    let fixture = Fixture::new();
    let (probe, leases, marker) = (fixture.root.join("lease-probe.sh"), fixture.root.join("leases"), fixture.root.join("ran"));
    north_core::foundation::shim::write_executable(&probe, format!("#!/usr/bin/env bash\n{LEASE_PROBE}"));
    let capacity = format!("bash {}", probe.display());
    fixture.add_settings(&[("landing.capacity", &capacity), ("land.check", "** :: printf ran >\"$ENQUEUE_MARKER\"")], "Add a leased land check");
    let (output, _) = north_enqueue_with(&fixture, &[("LEASE_LOG", &leases), ("ENQUEUE_MARKER", &marker)]);
    assert!(output.status.success(), "{}", Fixture::output_of(&output));
    assert!(marker.is_file());
    assert_eq!(fs::read_to_string(&leases).unwrap(), "acquire\nrelease\n");
    assert_eq!(fixture.pending().len(), 1);
}

const PRIVATE_TERM: &str = "zebra-quokka-7731";

fn deny_list(fixture: &Fixture) -> PathBuf {
    let path = fixture.root.join("denylist.toml");
    fs::write(&path, format!("title = \"enqueue test deny-list\"\n[[rules]]\nid = \"fake-private-term\"\ndescription = \"fake private identity term\"\nregex = '''{PRIVATE_TERM}'''\n")).unwrap();
    path
}

#[test]
fn enqueue_refuses_a_lane_carrying_a_deny_listed_value_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("identity.txt"), format!("owner: {PRIVATE_TERM}\n")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add identity"]);
    let list = deny_list(&fixture);
    let (output, _) = north_enqueue_with(&fixture, &[("SAFE_PUSH_DENYLIST", &list)]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("denylist: fake-private-term"), "{stdout}{stderr}");
    assert!(stderr.contains("deny-list scan refused the landing") && stderr.contains("NOT queued"), "{stderr}");
    assert!(!stdout.contains(PRIVATE_TERM) && !stderr.contains(PRIVATE_TERM));
    assert!(!fixture.state.exists(), "a refused lane writes no ticket");
}

#[test]
fn enqueue_refuses_a_lane_a_secret_scan_flags_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let stub = fixture.root.join("stub-bin");
    fs::create_dir_all(&stub).unwrap();
    north_core::foundation::shim::write_executable(&stub.join("gitleaks"), "#!/bin/sh\nexit 1\n");
    let path = PathBuf::from(format!("{}:{}", stub.display(), std::env::var("PATH").unwrap_or_default()));
    let (output, _) = north_enqueue_with(&fixture, &[("PATH", &path)]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("secret scan refused the landing"), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!fixture.state.exists(), "a refused lane writes no ticket");
}

#[test]
fn enqueue_says_so_and_queues_when_no_deny_list_is_reachable() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let (output, _) = north_enqueue(&fixture);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("so private values were not scanned"), "{stdout}");
    assert_eq!(fixture.pending().len(), 1);
}

const COLLAPSIBLE: &str = "pub fn pick(a: bool, b: bool) -> u8 {\n    if a {\n        if b {\n            return 1;\n        }\n    }\n    0\n}\n";
const TIDY: &str = "pub fn pick(a: bool, b: bool) -> u8 {\n    u8::from(a && b)\n}\n";

impl Fixture {
    fn rust_lane(&self, source: &str) {
        let name = format!("planted{}", self.root.file_name().unwrap().to_string_lossy().chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_lowercase());
        fs::write(self.repo.join("Cargo.toml"), format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")).unwrap();
        fs::write(self.repo.join("Cargo.lock"), format!("# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")).unwrap();
        fs::create_dir_all(self.repo.join("src")).unwrap();
        fs::write(self.repo.join("src/lib.rs"), source).unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-qm", "Add a Rust crate"]);
    }
}

#[test]
fn enqueue_refuses_a_rust_lane_that_fails_clippy_with_its_error_blocks_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fixture.rust_lane(COLLAPSIBLE);
    let (output, _) = north_enqueue(&fixture);
    let text = Fixture::output_of(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("clippy refused the lane") && text.contains("error: this `if` statement can be collapsed") && text.contains("src/lib.rs:2:5"), "{text}");
    assert!(text.contains("NOT queued"), "{text}");
    assert!(!fixture.state.exists(), "a refused lane writes no ticket to the queue or the outbox");
}

#[test]
fn enqueue_queues_a_rust_lane_that_passes_clippy() {
    let fixture = Fixture::new();
    fixture.rust_lane(TIDY);
    let (output, _) = north_enqueue(&fixture);
    assert!(output.status.success(), "{}", Fixture::output_of(&output));
    assert_eq!(fixture.pending().len(), 1);
}

#[test]
fn enqueue_runs_no_clippy_for_a_lane_that_changes_no_rust_file() {
    let fixture = Fixture::new();
    fixture.rust_lane(COLLAPSIBLE);
    git(&fixture.repo, &["push", "-q", "origin", "HEAD:main"]);
    fixture.commit(&fixture.repo, "notes.txt", "Add notes");
    let (output, _) = north_enqueue(&fixture);
    assert!(output.status.success(), "{}", Fixture::output_of(&output));
    assert_eq!(fixture.pending().len(), 1);
}

fn github_push_url(fixture: &Fixture, archived: &str) -> PathBuf {
    git(&fixture.repo, &["remote", "set-url", "--push", "origin", "https://github.com/o/wake.git"]);
    fixture.commit(&fixture.repo, "notes.txt", "Add notes");
    let bin = fixture.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    north_core::foundation::shim::write_executable(&bin.join("gh"), format!("#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>{}\n{archived}\n", fixture.root.join("gh-calls").display()));
    PathBuf::from(format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
}

#[test]
fn enqueue_refuses_a_repository_github_reports_archived_and_writes_no_ticket() {
    let fixture = Fixture::new();
    let path = github_push_url(&fixture, "echo true");
    let (output, _) = north_enqueue_with(&fixture, &[("PATH", &path)]);
    let text = Fixture::output_of(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("GitHub reports o/wake as archived") && text.contains("NOT queued"), "{text}");
    assert_eq!(fs::read_to_string(fixture.root.join("gh-calls")).unwrap(), "api repos/o/wake --jq .archived\n");
    assert!(!fixture.state.exists(), "a refused lane writes no ticket to the queue or the outbox");
}

#[test]
fn enqueue_queues_when_github_cannot_say_whether_the_repository_is_archived() {
    let fixture = Fixture::new();
    let path = github_push_url(&fixture, "echo 'error connecting to api.github.com' >&2; exit 1");
    let (output, _) = north_enqueue_with(&fixture, &[("PATH", &path)]);
    assert!(output.status.success(), "{}", Fixture::output_of(&output));
    assert_eq!(fixture.pending().len(), 1);
}
