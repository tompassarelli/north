use std::sync::Arc;
use std::time::Duration;

use crate::land::queue::{self, Ticket};
use crate::land::wake::{Task, Wake};

const LIMIT: Duration = Duration::from_secs(60);

#[test]
fn the_idle_lander_wakes_on_a_ticket_written_to_its_queue_and_on_a_check_exiting_before_its_next_round() {
    let queue_directory = tempfile::tempdir().unwrap();
    let wake = Arc::new(Wake::default());
    let _watcher = super::watch(queue_directory.path(), &wake);
    queue::write(&queue_directory.path().join("1-lane.json"), &Ticket::default()).unwrap();
    assert!(wake.wait(LIMIT), "the lander slept through a ticket written to its queue");
    let mut check = std::process::Command::new("sleep").arg("0.2").spawn().unwrap();
    wake.ring_on_exit(check.id());
    assert!(wake.wait(LIMIT), "the lander slept through a check process exiting");
    assert!(check.try_wait().unwrap().is_some(), "the lander woke before the check exited");
    let answer = Task::spawn(&wake, || 7);
    assert!(wake.wait(LIMIT) && answer.is_finished(), "the lander slept through a check thread ending, or woke before its result was ready");
    assert_eq!(answer.join().unwrap(), 7);
}
