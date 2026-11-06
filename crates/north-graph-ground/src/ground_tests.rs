use std::time::{Duration, Instant};

use north_graph::{Graph, Term, Triple};

use crate::{Seed, ground_with};

const OBSERVED: &str = "59ff4fb9c2d24f619e93afe867761c1146740b3d";
const UNSEEN: &str = "0aa2d15297f1a6a2be5ba41e5bbf4c8a9d3a2f10";

fn tell(graph: &mut Graph, writer: &str, t1: &str, predicate: &str, t3: impl Into<Term>) {
    graph.tell(writer, Triple::new(Term::string(t1), Term::keyword(predicate), t3)).unwrap();
}

fn claim(graph: &mut Graph, session: &str, node: &str, commit: &str) {
    let done = Term::triple(Term::string(node), Term::keyword("done-at"), Term::string(commit));
    tell(graph, session, session, "claims", done);
}

fn built(path: &std::path::Path) {
    let mut graph = Graph::open(path).unwrap();
    tell(&mut graph, "mirror", "north#634", "title", Term::string("Verification"));
    tell(&mut graph, "mirror", "north#634", "state", Term::keyword("open"));
    for (node, text, ticked) in [("box:north#634:a", "A hung slot job is killed", true), ("box:north#634:b", "The gate stops waiting", true), ("box:north#634:c", "Open work", false)] {
        tell(&mut graph, "mirror", "north#634", "has-box", Term::string(node));
        tell(&mut graph, "mirror", node, "text", Term::string(text));
        tell(&mut graph, "mirror", node, "ticked", Term::boolean(ticked));
    }
    claim(&mut graph, "n634-hung-slots", "box:north#634:a", OBSERVED);
    claim(&mut graph, "n634-gate", "box:north#634:b", UNSEEN);
    tell(&mut graph, "checker", OBSERVED, "check-passed", Term::string("safe-push landing"));
    tell(&mut graph, "ledger", "n634-hung-slots", "works-on", Term::string("north#634"));
    tell(&mut graph, "ledger", "north#634", "landed-as", Term::string(OBSERVED));
    tell(&mut graph, "push-log", OBSERVED, "landed-by", Term::string("n634-hung-slots"));
    tell(&mut graph, "push-log", OBSERVED, "landed-at", Term::instant(1_793_926_432, 0).unwrap());
    tell(&mut graph, "git", OBSERVED, "touched", Term::string("crates/north-capacity/src/slots.rs"));
    tell(&mut graph, "corrections", "correction:L1", "class", Term::string("class:unanswered"));
    tell(&mut graph, "corrections", "correction:L1", "paraphrase", Term::string("The reply did not answer the question."));
    tell(&mut graph, "corrections", "correction:L1", "in-session", Term::string("n634-hung-slots"));
}

fn ground(wanted: &[Seed]) -> String {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.db");
    built(&path);
    ground_with(&path, wanted, Some(Instant::now() + Duration::from_secs(5))).join("\n")
}

#[test]
fn the_neighborhood_reads_the_layout_ingest_stores_and_marks_claims_apart_from_observations() {
    let issue = ground(&[Seed::Issue("north#634".into())]);
    assert!(issue.contains("n634-hung-slots claims done at 59ff4fb: observed, checker passed `safe-push landing`"), "{issue}");
    assert!(issue.contains("n634-gate claims done at 0aa2d15: a claim, no checker observation"), "{issue}");
    assert!(issue.contains("open box: Open work"), "{issue}");
    assert!(issue.contains("recent landings: 59ff4fb by n634-hung-slots"), "{issue}");
    assert!(issue.contains("unanswered: The reply did not answer the question."), "{issue}");
    let file = ground(&[Seed::File("crates/north-capacity/src/slots.rs".into())]);
    assert!(file.contains("59ff4fb by n634-hung-slots for north#634"), "{file}");
    assert!(ground(&[Seed::Issue("north#999".into()), Seed::File("nowhere/x.rs".into())]).is_empty());
    let directory = tempfile::tempdir().unwrap();
    assert!(ground_with(&directory.path().join("none.db"), &[Seed::Issue("north#1".into())], None).is_empty());
}
