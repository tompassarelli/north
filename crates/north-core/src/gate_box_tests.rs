use std::sync::Arc;

use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::gates::{Answerer, GateBook, GateRequest};
use crate::fleet::registry::Fleet;
use crate::fleet::runtimes::Runtimes;

const GATED: &str = "- [ ] Rent a second build host for the lane checks. Done when: `north health` names it";
const OTHER: &str = "- [ ] Check each lane alone in parallel. Done when: `cargo nextest run -p north-landing lanes` exits 0";

#[test]
fn ticking_a_gated_box_closes_its_gate_and_a_box_scoped_gate_does_not_refuse_another_box() {
    let home = tempfile::tempdir().unwrap();
    let paper = Arc::new(Paper::default());
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), paper.clone()).unwrap().arming(|_| Ok(()));
    let request = GateRequest { verification: false, issue: Some("north#842".into()), box_text: Some("second build host".into()), session: "north-842-build-host".into(), question: "Which server?".into(), options: vec!["CPX62".into(), "CCX33".into()], recommend: 1, default: 1, default_after_ms: 8 * 3_600_000 };
    let id = paper.open_gate(&request, 1).unwrap();
    let launch = |line: &str| format!("Item: north#842 One bad lane must not sink a 14-lane batch\nBox: {}", line.trim_start_matches("- [ ] "));

    let holds = fleet.gate_holds().unwrap();
    assert!(holds.iter().any(|hold| hold.refuses("north#842", Some(&launch(GATED)))), "the gate holds the launch whose Box: line is its box");
    assert!(!holds.iter().any(|hold| hold.refuses("north#842", Some(&launch(OTHER)))), "nexus:36 refused a launch on another box of north#842 on 2026-11-08");

    let open = format!("{GATED}\n{OTHER}");
    assert!(fleet.close_ticked_gates(|_| Some(open.clone())).unwrap().is_empty(), "an unticked box keeps its gate open");

    let ticked = open.replacen("- [ ] Rent", "- [x] Rent", 1);
    let closed = fleet.close_ticked_gates(|issue| (issue == "north#842").then(|| ticked.clone())).unwrap();
    assert_eq!(closed.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![id]);
    let closure = paper.gate(id).unwrap().unwrap().closure.unwrap();
    assert_eq!(closure.by, Answerer::Ticked);
    assert!(closure.answer.contains("[x] Rent a second build host"), "the tick is the answer: {}", closure.answer);
    assert!(fleet.gate_holds().unwrap().is_empty(), "a closed gate holds no launch");
}
