fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn usage_errors_match_the_parsers_they_were_captured_from() {
    let spend: &[&[&str]] = &[&["--bogus"], &["stray"], &["--json", "--json"], &["--json=1"], &["--"], &["-x"], &["--json", "extra"]];
    for words in spend {
        assert_eq!(super::spend::run(&argv(words)), 2, "spend {words:?}");
    }
    let adopt: &[&[&str]] = &[&["--bogus"], &["stray"], &["--dry-run", "--dry-run"], &["--dry-run=1"], &["--"], &["-x"]];
    for words in adopt {
        assert_eq!(super::land_adopt::run(&argv(words)), 64, "land adopt {words:?}");
    }
    let check: &[&[&str]] = &[&["--tree"], &["--live"], &["stray"], &["--tree=DIR"], &["--tree", "a", "--live"], &["--tree", "a", "--bogus", "b"], &["--"], &["--tree", "a", "b"]];
    for words in check {
        assert_eq!(super::call_check::run(&argv(words), |_| true), 64, "check calls {words:?}");
    }
    let duplicate: &[&[&str]] = &[&["--bogus"], &["stray"], &["--days"], &["--days", "0"], &["--days", "-1"], &["--json=1"], &["--"], &["-x"], &["--days", "2", "extra"]];
    for words in duplicate {
        assert_eq!(super::ledger_duplicate::run(&argv(words)), 2, "ledger duplicate {words:?}");
    }
    let losses: &[&[&str]] = &[&["--bogus"], &["stray"], &["--since"], &["--since=24h"], &["--since=24h", "x"], &["--repo=north", "x"], &["--until", "not-a-date"], &["--until", "2026-11-15"], &["--until"], &["--since", "2w"], &["--"], &["-x"]];
    for words in losses {
        assert_eq!(super::scoreboard::losses::run(&argv(words)), 64, "dash losses {words:?}");
    }
    let worker: &[&[&str]] = &[&["--bogus"], &["stray"], &["--session"], &["--emit=1"], &["--"], &["-x"], &["--day"], &["--landed-issues"]];
    for words in worker {
        assert_eq!(super::worker_ledger::run(&argv(words)), 2, "ledger worker {words:?}");
    }

    let duplicate = super::ledger_duplicate::parse(&argv(&["--days", "2", "--days", "3", "--json"])).expect("ledger duplicate options");
    assert_eq!((duplicate.days, duplicate.json), (3, true));
    let default = super::ledger_duplicate::parse(&argv(&[])).expect("ledger duplicate defaults");
    assert_eq!((default.days, default.json), (1, false));

    let losses = super::scoreboard::losses::Request::parse(&argv(&["--repo", "south", "--repo", "north", "--since", "2h"])).expect("dash losses request");
    assert_eq!((losses.repo.as_str(), losses.since_s, losses.until), ("north", 7_200, None));
    let default = super::scoreboard::losses::Request::parse(&argv(&[])).expect("dash losses defaults");
    assert_eq!((default.repo.as_str(), default.since_s, default.until), ("north", 86_400, None));
    assert!(super::scoreboard::losses::Request::parse(&argv(&["--until", "2026-11-15T15:20:00Z"])).is_some_and(|losses| losses.until.is_some()));

    let emit = super::worker_ledger::parse(&argv(&["--emit", "--emit"])).expect("ledger worker emit");
    assert_eq!(emit.1, super::worker_ledger::Mode::Emit);
    let recommend = super::worker_ledger::parse(&argv(&["--recommend", "flow"])).expect("ledger worker recommend");
    assert_eq!(recommend.1, super::worker_ledger::Mode::Recommend { facets: "flow".to_owned() });
    let landed = super::worker_ledger::parse(&argv(&["--landed-issues", "acct", "--day", "2026-11-15"])).expect("ledger worker landed issues");
    assert_eq!(landed.1, super::worker_ledger::Mode::LandedIssues { account: "acct".to_owned(), day: "2026-11-15".to_owned() });
    let summary = super::worker_ledger::parse(&argv(&["--summary", "--by", "tier"])).expect("ledger worker summary");
    assert_eq!(summary.1, super::worker_ledger::Mode::Summary { by: "tier".to_owned() });
    let record = super::worker_ledger::parse(&argv(&["--session", "s1", "--since", "2d", "--backfill"])).expect("ledger worker record");
    assert_eq!((record.0.session.as_str(), record.0.since.as_str(), record.0.backfill), ("s1", "2d", true));
    assert_eq!(record.1, super::worker_ledger::Mode::Record);
}
