use super::*;
use crate::grounding::distill::{self, Row};

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
fn a_cited_decision_carries_its_summary_beside_the_id_and_an_unopenable_one_is_dropped() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = directory.path().join("ledger.jsonl");
    std::fs::write(&ledger, format!("{}\n", serde_json::to_string(&record()).unwrap())).unwrap();
    assert_eq!(cite("ledger:Ldemo", &ledger).as_deref(), Some("ledger:Ldemo — tom decision 2026-11-07: stop hook citations"));
    assert_eq!(cite("ledger:Lmissing", &ledger), None);
    assert_eq!(cite("Ldemo", &ledger), None);
    assert_eq!(record().owner(), "tom");
    assert_eq!(Row { source: String::new(), ..record() }.summary(), "unknown decision 2026-11-07: stop hook citations");
    assert_eq!(distill::show_at("ledger:Ldemo", std::path::Path::new("/nonexistent/ledger.jsonl")).unwrap(), None);
}
