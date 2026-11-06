use std::cell::Cell;

use north_dispatch::ledger::closed::checker_verified;

use crate::reward::{Reward, Winner, comparable, winner};

fn reward(usd: f64, kept: usize, verified: usize) -> Reward {
    Reward { judge: 2, usd, kept, verified }
}

#[test]
fn a_reward_counts_only_boxes_whose_every_tick_carries_a_passing_evidence_stamp_so_a_self_ticked_gain_never_wins_and_an_older_judge_is_rejudged() {
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
    let checked = "- [x] Done when tests pass. Evidence: `cargo test -p north-climb reward` exited 0 on main 1a2b3c4 on 2026-11-06.";
    assert!(checker_verified(checked));
    assert!(!checker_verified(&format!("{checked}\n- [x] Self-ticked by its worker (landed 1a2b3c4).")));
    assert!(!checker_verified("- [x] Done when x. Evidence: `cargo test` exited 1 on main 1a2b3c4 on 2026-11-06."));
    assert!(!checker_verified("- [ ] nothing ticked"));
}
