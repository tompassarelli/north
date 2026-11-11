use std::sync::Arc;
use std::time::Duration;

use super::super::outbox::Wake;
use crate::land::queue::{self, Ticket};

const LIMIT: Duration = Duration::from_secs(60);

#[test]
fn a_ticket_written_to_the_queue_wakes_the_idle_lander_before_its_next_round() {
    let queue_directory = tempfile::tempdir().unwrap();
    let wake = Arc::new(Wake::default());
    let _watcher = super::watch(queue_directory.path(), &wake);
    queue::write(&queue_directory.path().join("1-lane.json"), &Ticket::default()).unwrap();
    assert!(wake.wait(LIMIT), "the lander slept through a ticket written to its queue");
}
