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
