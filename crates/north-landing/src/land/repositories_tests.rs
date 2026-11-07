use std::path::PathBuf;

use super::*;
use crate::land::hold;

#[test]
fn a_queue_its_lander_holds_is_served_though_every_lane_worktree_its_tickets_name_is_gone() {
    let state = tempfile::tempdir().unwrap();
    let queue = state.path().join("north/land-queue/de2e0e8479546ba9");
    let gone = Ticket { branch: "landed-lane".to_owned(), host: "here".to_owned(), root: PathBuf::from("/nonexistent/worktrees/landed-lane"), queued_micros: Some(1_000), ..Default::default() };
    queue::write(&queue.join(queue::file_name(1_000, "landed-lane")), &gone).unwrap();
    assert_eq!(discovered(state.path(), "here").unserved.len(), 1, "an unheld queue whose tickets name no checkout is unserved");
    let _hold = hold::take(&queue, state.path()).unwrap();
    assert_eq!(discovered(state.path(), "here").unserved, Vec::<String>::new());
}
