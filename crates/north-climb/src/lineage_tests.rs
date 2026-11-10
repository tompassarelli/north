use std::cell::Cell;

use north_dispatch::ledger::closed::{ArmBox, checker_verified};
use north_dispatch::threads::claims::box_node;
use north_graph::{Graph, Term, Triple};

use crate::candidate::{Candidate, Change};
use crate::lineage::{self, Entry, ROOT, Status};
use crate::loss::Loss;
use crate::reward::{Reward, Winner, comparable, winner};

const COMMAND: &str = "cargo test -p north-climb lineage";

fn claimed(graph: &mut Graph, item: &str, text: &str, sha: &str) -> String {
    let node = box_node(item, text).unwrap();
    graph.tell("worker-1", Triple::new(Term::string("worker-1"), Term::keyword("claims"), Term::triple(Term::string(&node), Term::keyword("done-at"), Term::string(sha)))).unwrap();
    graph.tell("mirror", Triple::new(Term::string(&node), Term::keyword("done-when"), Term::string(COMMAND))).unwrap();
    node
}

fn checked(graph: &mut Graph, sha: &str) {
    graph.tell("checker", Triple::new(Term::string(sha), Term::keyword("check-passed"), Term::string(COMMAND))).unwrap();
}

fn kept(item: &str, text: &str) -> ArmBox {
    ArmBox { item: item.to_owned(), tier: "opus".to_owned(), arm: "lean".to_owned(), usd: 6.0, category: "feature".to_owned(), closed: true, reworked: false, verified: true, issue_verified: true, box_text: Some(text.to_owned()) }
}

fn candidate(id: &str, parent: &str) -> Candidate {
    Candidate { id: id.to_owned(), parent: parent.to_owned(), change: Change::ArmRoute { category: "feature".to_owned(), from: "base".to_owned(), to: "lean".to_owned(), to_tier: "opus".to_owned() }, why: String::new(), saving_usd: None }
}

#[test]
fn lineage_scores_a_candidate_only_from_boxes_the_checker_corroborated_by_its_landing_seq_so_a_self_ticked_gain_never_wins_and_draws_parents_across_the_archive() {
    let mut graph = Graph::open_in_memory().unwrap();
    claimed(&mut graph, "north#1", "Done when the archive reads", "aaa");
    claimed(&mut graph, "north#2", "Done when the parent is drawn", "bbb");
    checked(&mut graph, "aaa");
    let boxes = [kept("north#1", "Done when the archive reads"), kept("north#2", "Done when the parent is drawn")];
    let loss = Loss { tier: "opus".to_owned(), category: "feature".to_owned(), repeat_usd: 4.0, unclosed_usd: 2.0, closed: 2, runs: 3 };
    let seq = graph.head().unwrap();
    let reward = lineage::reward(&boxes, "feature", "lean", &lineage::corroborated(&graph, seq).unwrap(), 2);
    assert_eq!((reward.kept, reward.verified), (2, 1), "north#2 is only its worker's claim and both boxes carry the ledger's verified flag");
    let first = candidate("arm-route-feature-base-lean", ROOT);
    let entry = Entry { candidate: &first, loss: &loss, status: Status::Arm, replay_refusals: 0, replay_reached: 3, reward: Some(reward) };
    let node = lineage::record(&mut graph, &entry, seq).unwrap();
    checked(&mut graph, "bbb");
    assert_eq!(lineage::corroborated(&graph, seq).unwrap().len(), 1, "the check that passed after the landing seq stays out of the candidate's view");
    assert_eq!(lineage::corroborated(&graph, graph.head().unwrap()).unwrap().len(), 2);
    let archive = lineage::archive(&graph).unwrap();
    assert_eq!(archive, vec![lineage::Node { id: node.clone(), parent: ROOT.to_owned(), status: "arm".to_owned(), landed_seq: seq, scored: Some(12.0) }]);
    let mutated = graph.show(&Term::string(&node)).unwrap().into_iter().filter(|live| live.triple.t2 == Term::keyword("mutates")).count();
    assert_eq!(mutated, 1);
    let drawn: Vec<String> = (0..64).map(|seed| lineage::draw(&archive, seed)).collect();
    assert!(drawn.contains(&node) && drawn.iter().any(|parent| parent == ROOT), "both the root and the arm node are drawn: {drawn:?}");
    let second = candidate("arm-route-feature-base-lean", &node);
    let later = graph.head().unwrap();
    lineage::record(&mut graph, &Entry { candidate: &second, loss: &loss, status: Status::Proposed, replay_refusals: 0, replay_reached: 3, reward: None }, later).unwrap();
    let archive = lineage::archive(&graph).unwrap();
    assert_eq!(archive.iter().map(|node| node.parent.as_str()).collect::<Vec<_>>(), vec![ROOT, node.as_str()]);
    assert!((0..64).map(|seed| lineage::draw(&archive, seed)).all(|parent| parent != lineage::node(&second.id, later)), "a proposed candidate never ran, so it is no parent");
    rewards_compare_only_checker_verified_boxes_under_one_judge();
}

fn rewards_compare_only_checker_verified_boxes_under_one_judge() {
    let reward = |usd: f64, kept: usize, verified: usize| Reward { judge: 2, usd, kept, verified };
    let parent = reward(100.0, 10, 6);
    assert_eq!(winner(&parent, &reward(100.0, 14, 6)), None, "four more self-ticked boxes for the same spend");
    assert_eq!(winner(&parent, &reward(100.0, 20, 4)), None, "cheaper per kept box but dearer per checker-verified one");
    assert_eq!(winner(&parent, &reward(100.0, 12, 8)), Some(Winner::Candidate));
    assert_eq!(winner(&reward(100.0, 12, 8), &parent), Some(Winner::Parent));
    assert_eq!(reward(100.0, 12, 8).self_ticked(), 4);
    let calls = Cell::new(0);
    let rejudge = |old: &Reward, judge: u32| {
        calls.set(calls.get() + 1);
        Reward { judge, kept: old.kept - 2, ..old.clone() }
    };
    let (old, new) = comparable(Reward { judge: 1, ..reward(100.0, 10, 6) }, reward(100.0, 10, 6), rejudge);
    assert_eq!((calls.get(), old.judge, old.kept, new.kept), (1, 2, 8, 10));
    let (_, _) = comparable(reward(1.0, 1, 1), reward(1.0, 1, 1), rejudge);
    assert_eq!(calls.get(), 1, "rewards under one judge are compared as taken");
    let checked = "- [x] Done when: `cargo test -p north-climb reward`. Evidence: `cargo test -p north-climb reward` exited 0 on main 1a2b3c4 on 2026-11-06.";
    assert!(checker_verified(checked));
    assert!(!checker_verified(&format!("{checked}\n- [x] Self-ticked by its worker (landed 1a2b3c4).")));
    assert!(!checker_verified("- [x] Done when: `cargo test`. Evidence: `cargo test` exited 1 on main 1a2b3c4 on 2026-11-06."));
    assert!(!checker_verified("- [x] Done when: `cargo test`. Evidence: `cargo nextest run` exited 0 on main 1a2b3c4 on 2026-11-06."), "another command's pass is not this box's Done-when passing");
    assert!(!checker_verified("- [ ] nothing ticked"));
}
