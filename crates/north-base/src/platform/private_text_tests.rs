use super::{JUDGE_SLOTS, slot, try_slot};

// KEEP(bug): on 2026-11-08 the lander's candidate checks of the 78d8e9c920 round started about 27 privacy judges at once on the laptop, each running its own model processes, 17 cores unleased while Tom worked on it (north#842)
#[test]
fn a_fifth_judge_finds_no_slot_while_four_hold_them_and_takes_one_a_judge_frees() {
    let directory = tempfile::tempdir().unwrap();
    let mut held: Vec<_> = (0..JUDGE_SLOTS).map(|_| slot(directory.path()).unwrap()).collect();
    assert!(try_slot(directory.path()).unwrap().is_none(), "a fifth judge started beside {JUDGE_SLOTS}");
    held.pop();
    assert!(try_slot(directory.path()).unwrap().is_some(), "a freed slot was not taken");
}
