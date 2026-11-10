use super::*;

fn record() -> Row {
    Row {
        id: "Ldemo".into(),
        at: "2026-11-07T03:39:20Z".into(),
        kind: "decision".into(),
        topic: "stop hook citations".into(),
        quote: "We go with the lander.".into(),
        source: "tom convo:7 session:s1".into(),
        superseded_by: None,
        class: None,
        corrected: None,
        digest: None,
    }
}

#[test]
fn a_record_prints_its_decision_date_owner_and_source() {
    assert_eq!(
        listing(&record()),
        "id: Ldemo\ndate: 2026-11-07T03:39:20Z\nowner: tom\nsource: tom convo:7 session:s1\ndecision: We go with the lander.\n"
    );
}

#[test]
fn the_citation_form_and_the_bare_record_id_both_open_and_anything_else_is_usage() {
    assert_eq!(record_id("ledger:Ldemo"), Some("Ldemo"));
    assert_eq!(record_id("Ldemo"), Some("Ldemo"));
    assert_eq!(record_id("demo"), None);
    assert_eq!(record_id("L"), None);
}

#[test]
fn a_missing_id_exits_1_and_a_recorded_one_exits_0() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = directory.path().join("ledger.jsonl");
    assert_eq!(run_at(&["Lmissing".to_owned()], &ledger), 1);
    std::fs::write(&ledger, format!("{}\n", serde_json::to_string(&record()).unwrap())).unwrap();
    assert_eq!(run_at(&["Ldemo".to_owned()], &ledger), 0);
}
