use super::*;

#[test]
fn the_list_carries_this_hosts_gates_beside_the_open_gates_other_hosts_hold_with_their_answerable_ids() {
    let request = |session: &str, issue: Option<&str>| GateRequest { verification: false, issue: issue.map(str::to_owned), box_text: None, session: session.into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 600_000 };
    let mine = Gate { id: 7, request: request("proxy", None), opened_ms: 1_000, closure: None };
    let theirs = GateRow { host: "nexus".into(), id: 4, request: request("fdl-probe", Some("north#416")), opened_ms: 2_000 };

    let rows = listed("laptop", &[mine], &[theirs]);

    let seen: Vec<(&str, String, &str)> = rows.iter().map(|row| (row["host"].as_str().unwrap(), row["id"].to_string(), row["session"].as_str().unwrap())).collect();
    assert_eq!(seen, [("laptop", "7".to_owned(), "proxy"), ("nexus", "\"nexus:4\"".to_owned(), "fdl-probe")]);
    assert_eq!((rows[1]["issue"].as_str(), rows[1]["default_at_ms"].as_u64(), rows[1]["closed_ms"].is_null()), (Some("north#416"), Some(602_000), true));
}

#[test]
fn open_reads_repeated_options_and_a_unit_duration() {
    let flags = ["--session", "w", "--question", "Rent?", "--option", "yes", "--option", "no", "--recommend", "1", "--default", "2", "--default-after", "30s"];
    let request = gate_request(&flags).unwrap();
    assert_eq!((request.options.as_slice(), request.recommend, request.default, request.default_after_ms), (&["yes".to_owned(), "no".to_owned()][..], 1, 2, 30_000));
    assert_eq!([duration_ms("10m"), duration_ms("4h"), duration_ms("1d"), duration_ms("5"), duration_ms("h")], [Some(600_000), Some(14_400_000), Some(86_400_000), None, None]);
    assert!(gate_request(&flags[..flags.len() - 2]).is_err(), "a gate without --default-after must be refused");
    let decided = [&flags[..], &["--issue", "tompassarelli/North#370"]].concat();
    assert_eq!(gate_request(&decided).unwrap().issue.as_deref(), Some("north#370"));
    assert!(gate_request(&[&flags[..], &["--issue", "370"]].concat()).is_err(), "an --issue without a repository must be refused");
    let scoped_flags = [&flags[..], &["--issue", "north#370", "--box", "ring crate"]].concat();
    assert_eq!(gate_request(&scoped_flags).unwrap().box_text.as_deref(), Some("ring crate"));
    assert!(gate_request(&[&flags[..], &["--box", "ring crate"]].concat()).is_err(), "a --box without an --issue scopes nothing");
    assert!(gate_request(&[&flags[..], &["--issue", "north#370", "--box", " "]].concat()).is_err(), "an empty --box must be refused");
    let Err(NorthError::Usage(help)) = gate_request(&["--help"]) else { panic!("--help prints the usage") };
    assert!(help.contains("--box TEXT"), "{help}");
    let north_help = include_str!("help.txt").lines().find(|line| line.trim_start().starts_with("gate [open")).expect("help.txt has a gate line");
    for flag in USAGE.split(" | ").next().unwrap().split_whitespace().map(|word| word.trim_start_matches('[')).filter(|word| word.starts_with("--")) {
        assert!(north_help.contains(flag), "north help's gate line lacks {flag}: {north_help}");
    }
    assert!(north_help.contains("[--default N --default-after D]"), "an ask-once gate takes no default, so north help marks it optional: {north_help}");
    let ask = ["--session", "w", "--question", "Encrypt the GitHub App credential to one more host?", "--option", "yes", "--option", "no", "--recommend", "1"];
    for defaults in [&["--default", "1", "--default-after", "4h"][..], &["--default", "1"], &["--default-after", "4h"]] {
        let Err(NorthError::Usage(refusal)) = gate_request(&[&ask[..], defaults].concat()) else { panic!("an ask-once gate with {defaults:?} must be refused") };
        assert!(refusal.contains("ask-once matter (credentials)"), "{refusal}");
    }
    let request = gate_request(&ask).unwrap();
    let gate = Gate { id: 3, request, opened_ms: 1_000, closure: None };
    let rows = listed("laptop", std::slice::from_ref(&gate), &[]);
    assert_eq!((rows[0]["question"].as_str(), rows[0]["default"].is_null(), rows[0]["default_at_ms"].is_null()), (Some("Encrypt the GitHub App credential to one more host?"), true, true));
    assert!(!due(&gate, u64::MAX), "a gate with no default never falls due");
    let plain = ["--session", "w", "--question", "Rename the module?", "--option", "yes", "--option", "no", "--recommend", "1"];
    assert!(gate_request(&plain).is_err(), "a question outside the ask-once table keeps its required default");
    let request = |box_text: Option<&str>| GateRequest { verification: false, issue: Some("north#657".into()), box_text: box_text.map(str::to_owned), session: "w".into(), question: "Rename?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 1, default_after_ms: 60_000 };
    let Err(NorthError::Usage(refusal)) = scoped(&request(None), |_| Ok(3)) else { panic!("a gate without --box on a 3-box issue must be refused") };
    assert!(refusal.contains("north#657 has 3 open boxes"), "{refusal}");
    assert!(scoped(&request(Some("scoreboard")), |_| panic!("a gate naming its box counts nothing")).is_ok());
    assert!(scoped(&request(None), |_| Ok(1)).is_ok(), "a single-box issue is held whole");
    let unscoped = GateRequest { issue: None, ..request(None) };
    assert!(scoped(&unscoped, |_| panic!("a gate on no issue reads no issue")).is_ok());
    let options = vec!["yes".to_owned(), "no".to_owned()];
    let class = |question: &str| crate::fleet::ask_once::class(question, &options);
    assert_eq!([class("Widen access to gex45 for the app?"), class("Delete the ledger?")], [Some("credentials"), Some("deletion")]);
    for routine in ["Delete the dead test helper?", "Which account should the retry run on?", "Make `parse` public?", "Raise the token budget to 350k?", "Is the CPU cost acceptable?", "Is the accountant happy?"] {
        assert_eq!(class(routine), None, "{routine} is a routine call that keeps its default");
    }
}
