#![cfg(test)]

use super::gate_digest::decide;
use crate::hook::Verdict;

#[test]
fn toms_window_starts_with_the_decision_queue_only_when_it_holds_something() {
    let queue = "For Tom (1): only the work each holds waits; everything else keeps going.\n- gate 169 (n562-w, open 2 h): Give each host a key?\n";
    let Verdict::Advise(text) = decide(true, || Some(queue.into())) else { panic!("a non-empty digest must reach Tom's window") };
    assert!(text.contains("gate 169") && text.contains("north gate digest"), "{text}");
    assert!(matches!(decide(true, || Some(String::new())), Verdict::Allow), "an empty queue prints nothing");
    assert!(matches!(decide(true, || None), Verdict::Allow), "a digest that fails or times out fails open");
    assert!(matches!(decide(false, || panic!("a worker never reads the digest")), Verdict::Allow));
}
