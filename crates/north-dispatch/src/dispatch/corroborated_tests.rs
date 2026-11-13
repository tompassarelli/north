use north_graph::{Graph, Term, Triple};

const SHA: &str = "59ff4fb9c2d24f619e93afe867761c1146740b3d";
const COMMAND: &str = "cargo test -p north-providers corroborated";
const BOX: &str = "Corroborated rewards: the staffed box of an umbrella";

fn run() -> crate::ledger::closed::SpendRun {
    crate::ledger::closed::SpendRun {
        agent: "worker".into(),
        item: Some("north#882".into()),
        role: "worker".into(),
        tier: "high".into(),
        category: "feature".into(),
        arm: "context-400k".into(),
        box_text: Some(BOX.into()),
        model: Some("claude-opus-5-5".into()),
        effort: String::new(),
        started: "2026-11-08T10:00:00Z".into(),
        ended: "2026-11-08T11:00:00Z".into(),
        landed: true,
        reworked: false,
        input: 1_000,
        cache_read: 0,
        output: 100,
        skills: Vec::new(),
    }
}

#[test]
fn a_box_closes_and_an_arm_is_rewarded_only_on_a_mechanical_assertion_corroborating_the_claim() {
    let node = crate::threads::claims::box_node("north#882", BOX).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.db");
    let mut graph = Graph::open(&path).unwrap();
    let mut tell = |writer: &str, subject: Term, relation: &str, object: Term| graph.tell(writer, Triple::new(subject, Term::keyword(relation), object)).unwrap();
    tell("worker", Term::string("worker"), "claims", Term::triple(Term::string(&node), Term::keyword("done-at"), Term::string(SHA)));
    tell("mirror", Term::string("north#882"), "has-box", Term::string(&node));
    tell("mirror", Term::string(&node), "done-when", Term::string(COMMAND));
    tell("push-log", Term::string(SHA), "landed-by", Term::string("worker"));
    let stamped = format!("- [x] {BOX}. Done when: `{COMMAND}` exits 0. Evidence: `{COMMAND}` passed on main 59ff4fb9c2d2 on 2026-11-08, exit 0.\n");
    let bodies: std::collections::HashMap<String, String> = [("north#882".to_owned(), stamped)].into();
    let rewarded = |graph: &std::path::Path| {
        let corroborated = crate::ledger::corroborated::checked_boxes(graph).unwrap();
        let items = bodies.keys().cloned().collect();
        let prices = crate::router::pricing::PriceTable::load().unwrap();
        crate::ledger::closed::summarize(&[run()], &items, &Default::default(), "2026-11-01T00:00:00Z", &prices).verified(&bodies, corroborated.as_ref()).arms[0].verified
    };
    assert!(!rewarded(&path), "a worker's quoted claim with no checker, lander or git assertion earns no reward");
    let mut graph = Graph::open(&path).unwrap();
    graph.tell("checker", Triple::new(Term::string(SHA), Term::keyword("check-passed"), Term::string(COMMAND))).unwrap();
    graph.tell("checker", Triple::new(Term::string(SHA), Term::keyword("checked-for"), Term::string("north#882"))).unwrap();
    graph.tell("checker", Triple::new(Term::string("north#882"), Term::keyword("passed-check"), Term::string(COMMAND))).unwrap();
    assert!(rewarded(&path), "the checker passing the box's Done-when at the claimed commit rewards the arm");
}
