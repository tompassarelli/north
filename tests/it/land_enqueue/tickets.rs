use std::fs;
use std::process::Command;

use north_landing::land::enqueue::{issue_numbers, pick_issue};
use north_landing::land::queue::RedFix;
use north_landing::landing::lane_squash::Squash;

use super::{Fixture, git, north_enqueue_arguments};

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
    let queued = fixture.enqueue_keeping(&fixture.repo, true);
    assert!(!queued.outbox);
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
