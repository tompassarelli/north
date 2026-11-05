use super::*;

fn columns() -> Columns {
    Columns { shared_files: 2, depth: 1, history_seen: 3, history_bad: 1, prior_tries: 0, main_overlap: 0, lines: 40, files: 3, commits: 1, rust: 2, lander: 0, nondocs: 3, areas: 1, fork_age_minutes: 12 }
}

fn seen(ticket: &str) -> Line {
    Line::Seen(Seen {
        ticket: ticket.into(),
        branch: "lane".into(),
        head: "a".repeat(40),
        at: 1_700_000_000,
        cells: Cells { conflict: "shared 1-2, first".into(), pass: "other paths, clean history".into() },
        columns: columns(),
        neighbours: vec!["other".into()],
        policy: "fixed".into(),
    })
}

fn decided(ticket: &str, class: Outcome) -> Line {
    Line::Decided(Decided { ticket: ticket.into(), class, route: Route::Precheck, batch_size: 1, policy: "fixed".into() })
}

fn record(id: u128, branch: &str, class: Class) -> Record {
    Record { id, branch: branch.into(), head: "b".repeat(40), class, decided: 0 }
}

#[test]
fn every_line_kind_round_trips_through_the_ledger_file_in_append_order() {
    let queue = tempfile::tempdir().unwrap();
    let lines = vec![
        seen("1-lane"),
        Line::Check(Check { candidate: "c".repeat(40), lanes: vec!["lane".into()], host: "gex45".into(), lease_cpus: 6, queued_secs: 1.5, run_secs: 90.0, checks: 4, verdict: Verdict::Green, phase: Phase::Precheck }),
        Line::Deferred(Deferred { ticket: "1-lane".into(), behind: vec!["other".into()] }),
        decided("1-lane", Outcome::Landed),
    ];
    for line in &lines {
        append(queue.path(), line).unwrap();
    }
    let ledger = read(queue.path()).unwrap();
    assert_eq!(ledger, Ledger { lines, skipped: 0 });
}

#[test]
fn a_line_is_one_json_object_tagged_by_kind_with_snake_case_names() {
    let queue = tempfile::tempdir().unwrap();
    append(queue.path(), &decided("1-lane", Outcome::Reverted)).unwrap();
    append(queue.path(), &Line::Check(Check { candidate: "c".into(), lanes: vec![], host: "h".into(), lease_cpus: 2, queued_secs: 0.0, run_secs: 1.0, checks: 1, verdict: Verdict::RedMain, phase: Phase::PostLand })).unwrap();
    let text = fs::read_to_string(path(queue.path())).unwrap();
    let rows: Vec<serde_json::Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!((rows[0]["kind"].as_str(), rows[0]["class"].as_str(), rows[0]["route"].as_str()), (Some("decided"), Some("reverted"), Some("precheck")));
    assert_eq!((rows[1]["verdict"].as_str(), rows[1]["phase"].as_str()), (Some("red_main"), Some("post_land")));
}

#[test]
fn reading_skips_a_torn_or_unknown_line_and_a_missing_file_is_an_empty_ledger() {
    let queue = tempfile::tempdir().unwrap();
    assert_eq!(read(queue.path()).unwrap(), Ledger::default());
    append(queue.path(), &seen("1-lane")).unwrap();
    fs::OpenOptions::new().append(true).open(path(queue.path())).unwrap().write_all(b"{\"kind\":\"seen\",\"tick\n{\"kind\":\"later\"}\n\n").unwrap();
    append(queue.path(), &decided("1-lane", Outcome::Landed)).unwrap();
    let ledger = read(queue.path()).unwrap();
    assert_eq!((ledger.lines.len(), ledger.skipped), (2, 2));
}

#[test]
fn a_reverted_decision_turns_a_landed_record_into_a_pass_failure_and_touches_no_other() {
    let queue = tempfile::tempdir().unwrap();
    append(queue.path(), &decided(&ticket_name(10, "fast/lane"), Outcome::Reverted)).unwrap();
    append(queue.path(), &decided(&ticket_name(30, "kept"), Outcome::Landed)).unwrap();
    let ledger = read(queue.path()).unwrap();
    let joined = join(vec![record(10, "fast/lane", Class::Landed), record(20, "fast/lane", Class::Landed), record(30, "kept", Class::Landed), record(10, "other", Class::Conflict)], &ledger);
    assert_eq!(joined.iter().map(|record| record.class).collect::<Vec<_>>(), [Class::Failed, Class::Landed, Class::Landed, Class::Conflict]);
}

#[test]
fn a_ticket_name_is_the_queue_file_stem_with_the_branch_slug() {
    assert_eq!(ticket_name(1793854654314295, "claude/land x"), "1793854654314295-claude-land-x");
    assert_eq!(queue::file_name(5, "a/b"), format!("{}.json", ticket_name(5, "a/b")));
}
