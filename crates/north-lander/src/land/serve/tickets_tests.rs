use std::sync::Arc;
use std::time::Duration;

use crate::land::queue::{self, Ticket};
use crate::land::wake::{Task, Wake};

const LIMIT: Duration = Duration::from_secs(3600);

#[test]
fn a_queued_ticket_lands_on_the_backstop_tick_when_the_wake_event_is_dropped_and_a_crate_split_sends_back_a_lane_on_a_moved_file_in_the_same_round() {
    let queue_directory = tempfile::tempdir().unwrap();
    let wake = Arc::new(Wake::default());
    let _watcher = super::watch(queue_directory.path(), &wake);
    queue::write(&queue_directory.path().join("1-lane.json"), &Ticket::default()).unwrap();
    assert!(wake.wait(LIMIT), "the lander slept through a ticket written to its queue");
    let mut check = std::process::Command::new("sh").args(["-c", "read _"]).stdin(std::process::Stdio::piped()).spawn().unwrap();
    wake.ring_on_exit(check.id());
    assert!(check.try_wait().unwrap().is_none(), "the check exited before the lander was told to watch it");
    drop(check.stdin.take());
    assert!(wake.wait(LIMIT), "the lander slept through a check process exiting");
    assert!(check.try_wait().unwrap().is_some(), "the lander woke before the check exited");
    let answer = Task::spawn(&wake, || 7);
    assert!(wake.wait(LIMIT) && answer.is_finished(), "the lander slept through a check thread ending, or woke before its result was ready");
    assert_eq!(answer.join().unwrap(), 7);
    let dropped_directory = tempfile::tempdir().unwrap();
    let unwired = Arc::new(Wake::default());
    let _quiet = super::start(dropped_directory.path(), || {});
    queue::write(&dropped_directory.path().join("2-lane.json"), &Ticket::default()).unwrap();
    assert!(!unwired.wait(super::super::POLL), "the lander woke on a ticket whose event was dropped");
    assert_eq!(queue::pending(dropped_directory.path()).unwrap().len(), 1, "the backstop round does not find the queued ticket");
    super::super::moved::tests::a_crate_split_sends_back_the_queued_lane_on_a_moved_file_in_the_same_round_naming_the_split();
}
