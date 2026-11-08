use crate::store::{Graph, TextSearch};
use crate::term::{Term, Triple};

fn doc(id: &str, predicate: &str, text: &str) -> Triple {
    Triple::new(Term::string(id), Term::keyword(predicate), Term::string(text))
}

fn subjects(graph: &Graph, query: &str) -> Vec<Term> {
    let search = TextSearch { query, field: None, weights: [4.0, 1.0], facets: &["doc-source"], cap: Some(1) };
    let mut found: Vec<Term> = graph.text_search(&search).unwrap().into_iter().map(|hit| hit.subject).collect();
    found.sort();
    found
}

#[test]
fn the_text_index_follows_every_assertion_and_retraction_and_a_v2_store_upgrades_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.db");
    let mut graph = Graph::open(&path).unwrap();
    for (id, source, title) in [("doc:a", "policy", "landing queue"), ("doc:b", "policy", "lander probes"), ("doc:c", "commits", "queued landings")] {
        graph.tell("grounding", doc(id, "doc-source", source)).unwrap();
        graph.tell("grounding", doc(id, "doc-title", title)).unwrap();
    }
    graph.tell("grounding", doc("doc:b", "doc-text", "the landing queue p90")).unwrap();
    graph.tell("other", doc("doc:b", "doc-text", "the landing queue p90")).unwrap();
    assert_eq!(subjects(&graph, "\"land\""), [Term::string("doc:a"), Term::string("doc:c")], "one hit per source, the title-weighted one for policy");
    graph.retract("grounding", doc("doc:a", "doc-title", "landing queue")).unwrap();
    graph.retract("grounding", doc("doc:b", "doc-text", "the landing queue p90")).unwrap();
    assert_eq!(subjects(&graph, "\"land\""), [Term::string("doc:b"), Term::string("doc:c")], "a text another writer still asserts stays indexed");
    graph.retract("other", doc("doc:b", "doc-text", "the landing queue p90")).unwrap();
    assert_eq!(subjects(&graph, "\"p90\""), Vec::<Term>::new());
    let connection = graph.connection();
    connection.execute("DELETE FROM meta WHERE key = 'text-index'", []).unwrap();
    connection
        .execute_batch("CREATE TABLE terms_v2 (id INTEGER PRIMARY KEY, key BLOB NOT NULL UNIQUE); INSERT INTO terms_v2 SELECT id, key FROM terms; DROP TABLE terms; ALTER TABLE terms_v2 RENAME TO terms; PRAGMA user_version = 2;")
        .unwrap();
    drop(graph);
    let mut graph = Graph::open(&path).unwrap();
    assert!(graph.validate().unwrap().is_sound());
    graph.tell("grounding", doc("doc:b", "doc-title", "lander probes")).unwrap();
    let title = graph.show(&Term::string("doc:b")).unwrap().into_iter().find(|live| live.triple.t2 == Term::keyword("doc-title")).unwrap();
    assert_eq!(title.occurrences, 2, "a v2 store's terms resolve by hash after the upgrade, so the told title is the stored one");
    assert_eq!(graph.text_rows().unwrap(), 2);
    assert_eq!(subjects(&graph, "\"probe\""), [Term::string("doc:b")]);
    let reader = Graph::open_read_only(&path, std::time::Duration::from_secs(1)).unwrap();
    assert_eq!(reader.text_stems(&["landings", "probes"]).unwrap(), [Some("land".to_string()), Some("probe".to_string())]);
    assert_eq!(reader.text_frequencies(&["queu"]).unwrap().get("queu"), Some(&1));
}
