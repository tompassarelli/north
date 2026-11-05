use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::*;
use crate::land_model::buckets::Target;
use crate::land::queue::{self, Ticket};

fn record(branch: &str, queued: i64, decided: i64, class: Class) -> Record {
    Record { id: queued as u128 * 1_000_000, branch: branch.into(), head: "0".repeat(40), class, decided }
}

fn changed(files: &[&str]) -> Change {
    Change { files: files.iter().map(|file| (*file).to_owned()).collect(), lines: 10, commits: 1, fork_time: 0, main_overlap: 0 }
}

fn entry(branch: &str, queued: i64, decided: i64, class: Class, files: &[&str]) -> (Record, Change) {
    (record(branch, queued, decided, class), changed(files))
}

#[test]
fn a_resubmits_own_earlier_ticket_adds_nothing_to_neighbour_overlap_or_history() {
    let entries = [
        entry("lane", 1000, 1500, Class::Conflict, &["crates/a/src/x.rs", "crates/a/src/y.rs"]),
        entry("lane", 2000, 2500, Class::Landed, &["crates/a/src/x.rs", "crates/a/src/y.rs"]),
        entry("lane", 1800, 2600, Class::Failed, &["crates/a/src/x.rs"]),
    ];
    let rows = rows(&entries);
    let second = &rows[1];
    assert_eq!((second.columns.shared_files, second.columns.depth, second.columns.history_seen, second.columns.history_bad), (0, 0, 0, 0));
    assert_eq!(second.columns.prior_tries, 2);
    assert!(second.lane.resubmit);
    assert!(!second.lane.history_refused);
    assert_eq!(rows[0].columns.prior_tries, 0);
    assert!(!rows[0].lane.resubmit);
    assert_eq!(Target::Conflict.cell(&second.lane), 1);
}

#[test]
fn another_branchs_in_flight_ticket_adds_its_shared_files_and_its_decided_ticket_adds_history() {
    let entries = [
        entry("mine", 2000, 3000, Class::Landed, &["a.rs", "b.rs", "c.rs"]),
        entry("flying", 1500, 2600, Class::Landed, &["a.rs", "b.rs", "z.rs"]),
        entry("refused", 500, 900, Class::Failed, &["c.rs"]),
        entry("clean", 600, 1000, Class::Landed, &["c.rs", "b.rs"]),
        entry("unrelated", 700, 1100, Class::Failed, &["q.rs"]),
        entry("later", 2500, 2800, Class::Landed, &["a.rs"]),
    ];
    let mine = &rows(&entries)[0];
    assert_eq!((mine.columns.shared_files, mine.columns.depth), (2, 1));
    assert_eq!((mine.columns.history_seen, mine.columns.history_bad), (2, 1));
    assert!(mine.lane.history_refused);
    assert_eq!(mine.lane.shared_files, 2);
}

#[test]
fn replaced_tickets_of_other_branches_are_neither_neighbours_nor_history() {
    let entries = [entry("mine", 2000, 3000, Class::Landed, &["a.rs"]), entry("old", 1500, 2600, Class::Replaced, &["a.rs"]), entry("older", 100, 200, Class::Replaced, &["a.rs"])];
    let mine = &rows(&entries)[0];
    assert_eq!((mine.columns.shared_files, mine.columns.depth, mine.columns.history_seen), (0, 0, 0));
}

#[test]
fn columns_count_docs_only_lander_paths_rust_and_areas() {
    let entries = [
        entry("docs", 10, 20, Class::Landed, &["docs/rationale/x.md", "README.md"]),
        entry("lander", 10, 20, Class::Landed, &["crates/north-landing/src/land/queue.rs", "bin/safe-push", "docs/y.md", "Cargo.lock"]),
        entry("empty", 10, 20, Class::Landed, &[]),
    ];
    let rows = rows(&entries);
    assert!(rows[0].lane.docs_only && !rows[0].lane.lander_paths);
    assert_eq!((rows[0].columns.nondocs, rows[0].columns.areas, rows[0].columns.rust), (0, 1, 0));
    assert!(!rows[1].lane.docs_only && rows[1].lane.lander_paths);
    assert_eq!((rows[1].columns.lander, rows[1].columns.rust, rows[1].columns.areas, rows[1].columns.nondocs), (2, 2, 3, 3));
    assert!(!rows[2].lane.docs_only);
}

#[test]
fn a_lanes_era_follows_when_it_was_decided() {
    let before = entry("a", 1, super::super::PRECHECK_ERA_START - 1, Class::Landed, &["x.rs"]);
    let after = entry("b", 1, super::super::PRECHECK_ERA_START, Class::Landed, &["x.rs"]);
    let rows = rows(&[before, after]);
    assert_eq!((rows[0].lane.era, rows[1].lane.era), (Era::Batch, Era::Precheck));
}

#[test]
fn a_verdict_names_its_class_and_a_failed_log_with_an_infra_marker_is_infra() {
    let log = |text: &'static str| move |_: &Path| Some(text.to_owned());
    assert_eq!(classify("landed abc", |_| None), Some(Class::Landed));
    assert_eq!(classify("conflict a.rs b.rs", |_| None), Some(Class::Conflict));
    assert_eq!(classify("replaced by a newer ticket for this lane", |_| None), Some(Class::Replaced));
    assert_eq!(classify("failed /tmp/x.log", log("tests red")), Some(Class::Failed));
    assert_eq!(classify("failed /tmp/x.log", log("step exited 255")), Some(Class::Infra));
    assert_eq!(classify("failed /tmp/x.log", |_| None), Some(Class::Failed));
    assert_eq!(classify("withdrawn", |_| None), None);
    assert_eq!(classify("", |_| None), None);
}

fn git(directory: &Path, date: Option<&str>, arguments: &[&str]) -> String {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(arguments);
    if let Some(date) = date {
        command.env("GIT_AUTHOR_DATE", date).env("GIT_COMMITTER_DATE", date);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(directory: &Path, date: &str, files: &[(&str, &str)]) -> String {
    for (name, text) in files {
        fs::write(directory.join(name), text).unwrap();
    }
    git(directory, None, &["add", "-A"]);
    git(directory, Some(date), &["commit", "-q", "-m", "work"]);
    git(directory, None, &["rev-parse", "HEAD"])
}

fn fixture() -> (tempfile::TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    git(path, None, &["init", "-q", "-b", "main"]);
    commit(path, "@1700000000 +0000", &[("README.md", "base\n")]);
    git(path, None, &["checkout", "-q", "-b", "lane"]);
    let head = commit(path, "@1700000100 +0000", &[("a.rs", "1\n2\n3\n"), ("b.rs", "1\n2\n")]);
    git(path, None, &["checkout", "-q", "main"]);
    commit(path, "@1700000200 +0000", &[("a.rs", "main\n")]);
    commit(path, "@1700005000 +0000", &[("b.rs", "main\n")]);
    (directory, head)
}

#[test]
fn change_reads_files_lines_commits_and_the_main_overlap_at_the_time_it_was_queued() {
    let (directory, head) = fixture();
    let record = Record { head, ..record("lane", 1700002000, 1700002500, Class::Landed) };
    let change = super::change(directory.path(), "main", &record).unwrap();
    assert_eq!(change.files, BTreeSet::from(["a.rs".to_owned(), "b.rs".to_owned()]));
    assert_eq!((change.lines, change.commits, change.fork_time, change.main_overlap), (5, 1, 1700000000, 1));
    let later = Record { id: 1700006000 * 1_000_000, ..record };
    assert_eq!(super::change(directory.path(), "main", &later).unwrap().main_overlap, 2);
}

#[test]
fn collect_skips_a_ticket_whose_head_the_repository_lacks_and_keys_the_rest_by_branch() {
    let (directory, head) = fixture();
    let records = vec![Record { head: head.clone(), ..record("lane", 2000, 2500, Class::Conflict) }, Record { head, ..record("lane", 3000, 3500, Class::Landed) }, record("ghost", 2100, 2200, Class::Landed)];
    let collected = collect(directory.path(), "main", records);
    assert_eq!(collected.skipped, 1);
    assert_eq!(collected.rows.len(), 2);
    assert_eq!((collected.rows[1].columns.shared_files, collected.rows[1].columns.history_seen, collected.rows[1].columns.prior_tries), (0, 0, 1));
    assert_eq!(lanes(&collected.rows).len(), 2);
}

#[test]
fn load_done_reads_tickets_named_by_queue_micros_and_skips_unknown_verdicts() {
    let directory = tempfile::tempdir().unwrap();
    let write = |name: &str, verdict: Option<&str>| {
        let ticket = Ticket { branch: name.into(), head: "a".repeat(40), verdict: verdict.map(str::to_owned), ..Default::default() };
        fs::write(directory.path().join(queue::file_name(2_000_000_000, name)), serde_json::to_string(&ticket).unwrap()).unwrap();
    };
    write("b-landed", Some("landed abc"));
    write("a-conflict", Some("conflict x.rs"));
    write("odd", Some("withdrawn"));
    write("open", None);
    fs::write(directory.path().join("notes.txt"), "x").unwrap();
    let records = load_done(directory.path()).unwrap();
    assert_eq!(records.iter().map(|record| (record.branch.as_str(), record.class)).collect::<Vec<_>>(), [("a-conflict", Class::Conflict), ("b-landed", Class::Landed)]);
    assert!(records.iter().all(|record| record.id == 2_000_000_000 && record.queued() == 2000));
}
