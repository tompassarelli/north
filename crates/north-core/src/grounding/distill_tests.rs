use super::*;

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
