use super::{JUDGE_SLOTS, slot, try_slot};

#[test]
fn a_fifth_judge_finds_no_slot_while_four_hold_them_and_takes_one_a_judge_frees() {
    let directory = tempfile::tempdir().unwrap();
    let mut held: Vec<_> = (0..JUDGE_SLOTS).map(|_| slot(directory.path()).unwrap()).collect();
    assert!(try_slot(directory.path()).unwrap().is_none(), "a fifth judge started beside {JUDGE_SLOTS}");
    held.pop();
    assert!(try_slot(directory.path()).unwrap().is_some(), "a freed slot was not taken");
}
