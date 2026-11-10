use std::path::PathBuf;

use super::*;
use crate::land::hold;

#[test]
fn a_queue_its_lander_holds_is_served_though_every_lane_worktree_its_tickets_name_is_gone() {
    let state = tempfile::tempdir().unwrap();
    let queue = state.path().join("north/land-queue/de2e0e8479546ba9");
    let gone = Ticket { branch: "landed-lane".to_owned(), host: "here".to_owned(), root: PathBuf::from("/nonexistent/worktrees/landed-lane"), queued_micros: Some(1_000), ..Default::default() };
    queue::write(&queue.join(queue::file_name(1_000, "landed-lane")), &gone).unwrap();
    let stray = state.path().join("north/land-queue/8733aa0a99314382");
    std::fs::create_dir_all(stray.join("logs")).unwrap();
    std::fs::write(stray.join("logs/spans.jsonl"), "").unwrap();
    std::fs::create_dir_all(stray.join("watchdog")).unwrap();
    std::fs::write(stray.join("watchdog/rounds.json"), "{}").unwrap();
    assert_eq!(discovered(state.path(), "here").unserved.len(), 1, "an unheld queue whose tickets name no checkout is unserved");
    assert_eq!(discovered(state.path(), "here").orphaned.as_slice(), std::slice::from_ref(&stray), "a queue with only logs and watchdog rounds is orphaned");
    queue::write(&stray.join("done").join(queue::file_name(1_001, "landed-lane")), &gone).unwrap();
    assert!(discovered(state.path(), "here").orphaned.is_empty(), "a queue with a decided ticket keeps its history");
    std::fs::remove_dir_all(stray.join("done")).unwrap();
    let _hold = hold::take(&queue, state.path()).unwrap();
    assert_eq!(discovered(state.path(), "here").unserved, Vec::<String>::new());
    assert_eq!(discovered(state.path(), "here").orphaned.as_slice(), std::slice::from_ref(&stray));
}
