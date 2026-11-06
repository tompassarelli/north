use super::*;

#[test]
fn rid_and_iso_match_the_typescript_forms() {
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(rid("convo:sess:/t/a.jsonl:3"), format!("L{}", &sha1_hex(b"convo:sess:/t/a.jsonl:3")[..10]));
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso(), "2026-10-15T10:52:55Z");
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso_millis(), "2026-10-15T10:52:55.752Z");
}

#[test]
fn a_fleet_message_relayed_into_a_session_is_never_read_as_tom_typing() {
    assert!(is_skip_message("[from proxy] Fact, no order: the lane is innocent."));
    assert!(is_skip_message("[from service:north-land] landed w230"));
    assert!(is_skip_message("Another Claude session sent a message: [from n634-lander-doctor] three updates"));
    assert!(is_skip_message("Item: north#656 a box\nCategory: feature") && is_brief_session("Item: north#656 a box\nCategory: feature"));
    assert!(!is_skip_message("from now on file every plan as an issue"));
}

#[test]
fn a_line_tom_quotes_from_the_reply_before_is_not_read_as_his_words() {
    let pasted = "<pasted_content id=\"70dc\">\nthe report he pasted\n</pasted_content id=\"70dc\">\n\nwhy did you let this slip";
    assert_eq!(typed_text(&own_words(pasted, None)), "why did you let this slip");
    let reply = "The alternative is staying here: you keep every feature, but get no resume.";
    let typed = "The alternative is staying here: you keep every feature, but get no resume.\n\nis this ready yet?";
    assert_eq!(typed_text(&own_words(typed, Some(reply))), "is this ready yet?");
    assert_eq!(classify(&typed_text(&own_words(typed, Some(reply))), Some(reply)).0, None);
}

#[test]
fn a_pasted_agent_sentence_is_no_correction_and_each_correction_shape_gets_its_class() {
    assert_ne!(classify("you keep every feature, but get no resume or failover", None).0, Some(Kind::Correction));
    assert_eq!(classify("you keep asking me for decisions you could make", None).0, Some(Kind::Correction));
    for (typed, class) in [
        ("you are making me the alarm bot for the build", "tom-as-monitor"),
        ("the branch was red for 40 minutes, missed again", "tom-as-monitor"),
        ("how did you let that slip", "dropped-thread"),
        ("that footer is totally unnecessary", "noise-in-report"),
        ("what quota are you talking about", "unproven-cause"),
        ("the pace rule should go", "pace"),
    ] {
        assert_eq!(correction_class(typed).map(|(name, _)| name), Some(class), "{typed}");
        assert_eq!(classify(typed, None).0, Some(Kind::Correction), "{typed}");
    }
}

fn message(msg: i64, text: &str, previous: &str) -> Message {
    Message { msg, path: "/t/a.jsonl".into(), line: msg, ts: "2026-11-06T02:00:00.000Z".into(), role: "user".into(), text: text.into(), session: "s1".into(), previous: Some(previous.into()) }
}

#[test]
fn a_captured_correction_keeps_its_class_paraphrase_and_the_reply_it_corrected_never_toms_words() {
    let typed = "you're working way too slowly on north#634, pick it up";
    let reply = "I'll check the lander queue and report back on the stalled lane.";
    let mut rows = convo_records(&[message(7, typed, reply)]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].class.as_deref(), Some("pace"));
    assert_eq!(rows[0].corrected.as_deref(), Some(reply));
    let ids: HashSet<String> = rows.iter().map(|r| r.id.clone()).collect();
    link_supersession(&mut rows, &ids);
    let quote = rows[0].quote.clone();
    into_paraphrase(&mut rows[0], &quote);
    assert_eq!(rows[0].quote, "Tom said the work was moving too slowly. (about north#634)");
    assert_eq!(rows[0].topic, "pace");
    assert_eq!(rows[0].digest.as_deref(), Some(digest(typed).as_str()));
    let line = serde_json::to_string(&rows[0]).unwrap();
    assert!(!line.contains("way too slowly") && !line.contains("pick it up"), "{line}");
}

#[test]
fn two_corrections_of_one_class_in_one_session_stay_two_rows_after_paraphrase() {
    let first = convo_records(&[message(7, "you're going too slow", "a")]).remove(0);
    let second = convo_records(&[message(9, "this is too slow, not keeping up", "b")]).remove(0);
    assert_ne!(said_key(&first), said_key(&second));
    let mut paraphrased = first.clone();
    let quote = paraphrased.quote.clone();
    into_paraphrase(&mut paraphrased, &quote);
    assert_eq!(said_key(&paraphrased), said_key(&first));
}

#[test]
fn backfill_paraphrases_an_old_rule_row_and_keeps_a_hand_paraphrase_while_adding_the_corrected_reply() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("convo.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TABLE body(id INTEGER PRIMARY KEY, text TEXT); CREATE TABLE msg(id INTEGER PRIMARY KEY, file_id INT, line INT, role TEXT, body INT);
         INSERT INTO body VALUES (1, 'Landing is queued behind two lanes.'), (2, 'you are moving too slow'), (3, 'Here is the plan.'), (4, 'that is under-specified');
         INSERT INTO msg VALUES (10, 1, 1, 'assistant', 1), (11, 1, 2, 'user', 2), (12, 1, 3, 'assistant', 3), (13, 1, 4, 'user', 4);",
    )
    .unwrap();
    let old = |msg: i64, quote: &str, class: Option<&str>| Row {
        id: format!("L{msg}"),
        at: "2026-11-05T20:00:00Z".into(),
        kind: "correction".into(),
        topic: "words".into(),
        quote: quote.into(),
        source: format!("tom convo:{msg} session:s1"),
        superseded_by: None,
        class: class.map(str::to_owned),
        corrected: None,
        digest: None,
    };
    let mut rows = vec![old(11, "you are moving too slow", None), old(13, "Tom said the gate was not grounded.", Some("ungrounded-design"))];
    assert_eq!(backfill(&db, &mut rows).unwrap(), 2);
    assert_eq!((rows[0].quote.as_str(), rows[0].class.as_deref()), ("Tom said the work was moving too slowly.", Some("pace")));
    assert_eq!(rows[0].corrected.as_deref(), Some("Landing is queued behind two lanes."));
    assert_eq!(rows[1].quote, "Tom said the gate was not grounded.");
    assert_eq!(rows[1].corrected.as_deref(), Some("Here is the plan."));
    assert!(rows.iter().all(|r| r.digest.is_some()));
    assert_eq!(backfill(&db, &mut rows).unwrap(), 0);
}
