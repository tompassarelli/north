use super::*;

/// The grounding query reads its documents from graph.db: a doc told under the grounding writer
/// is ranked out of the graph's text index, and no `grounding/index.db` exists beside it. A query
/// that went back to a second store would return no hit here.
#[test]
fn the_query_reads_a_grounding_document_told_to_graph_db() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.db");
    let doc = Doc {
        source: "policy".to_owned(),
        id: "landing".to_owned(),
        reference: "docs/landing-queue.md".to_owned(),
        title: "Landing queue".to_owned(),
        text: "the landing test queue waits for a probe".to_owned(),
        at: 1.0,
        decays: false,
    };
    {
        let mut graph = Graph::open(&path).unwrap();
        for fact in doc.facts() {
            graph.tell(WRITER, fact).unwrap();
        }
    }
    let mut index = Index::open_at(&path).unwrap();
    assert_eq!(index.docs().unwrap(), 1);
    let (hits, _) = search("landing queue probe", &mut index, 2.0, 8, 0.0).unwrap();
    assert_eq!(hits.first().map(|hit| hit.reference.as_str()), Some("docs/landing-queue.md"));
    // `north graph ground query test` is the smoke check north#882 names: the positional word is the
    // query, and an explicit one-word query is answered rather than silenced as a typed prompt is.
    assert_eq!(query_text(&["query".to_owned(), "test".to_owned()]), "test");
    assert_eq!(query_text(&["query".to_owned(), "--tier".to_owned(), "B".to_owned(), "landing queue".to_owned()]), "landing queue");
    let text = compose::block_at(&path, "test", "A", 2.0, "").unwrap();
    assert!(text.contains("docs/landing-queue.md"), "the one-word query answers from graph.db: {text:?}");
    assert!(!directory.path().join("index.db").exists(), "the query creates no separate grounding index");
}
