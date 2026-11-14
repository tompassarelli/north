use std::cell::RefCell;

use super::{Pair, Reading, compare};

fn pair(base: [Reading; 2], candidate: [Reading; 2]) -> Pair {
    let runs = RefCell::new(vec![base[0].clone(), candidate[0].clone(), base[1].clone(), candidate[1].clone()].into_iter());
    compare(&|_: &str| runs.borrow_mut().next().unwrap(), "base", "candidate").pair
}

#[test]
fn a_pair_regresses_only_when_the_base_passes_twice_and_the_candidate_fails_twice_the_same_way() {
    let (green, red, other) = (Reading::Green("0".into()), Reading::Red("1".into()), Reading::Red("2".into()));
    let fault = Reading::NoVerdict("exited 75: no whole runtime".into());
    assert_eq!(pair([green.clone(), green.clone()], [red.clone(), red.clone()]), Pair::Regressed);
    assert_eq!(pair([red.clone(), red.clone()], [red.clone(), red.clone()]), Pair::SameRed, "red at both the base and the candidate blames neither");
    assert_eq!(pair([red.clone(), red.clone()], [other.clone(), other.clone()]), Pair::RedChanged, "a candidate red another way than its red base is told apart from the same red");
    assert_eq!(pair([green.clone(), fault], [red.clone(), red.clone()]), Pair::NoVerdict, "a faulted run is no verdict, never a pass");
    assert_eq!(pair([green.clone(), red.clone()], [red.clone(), red.clone()]), Pair::TwoAnswers, "a base that answers twice differently blames nothing");
    assert_eq!(pair([green.clone(), green], [red, other]), Pair::TwoAnswers, "a candidate that fails two different ways blames nothing");
}
