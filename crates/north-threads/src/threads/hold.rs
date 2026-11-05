//! A cap on how long one ingest keeps the ledger: past the cap a busy ledger is refused instead of
//! waited on, so the ingest fails uncommitted and the caller answers the retry code.
use std::cell::Cell;
use std::time::{Duration, Instant};

use rusqlite::Connection;

const POLL: Duration = Duration::from_millis(5);
const UNBOUNDED_ATTEMPTS: i32 = 12_000;

thread_local! {
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn wait(attempt: i32) -> bool {
    if DEADLINE.get().is_some_and(|deadline| Instant::now() >= deadline) || attempt >= UNBOUNDED_ATTEMPTS {
        return false;
    }
    std::thread::sleep(POLL);
    true
}

pub fn past_deadline() -> bool {
    DEADLINE.get().is_some_and(|deadline| Instant::now() >= deadline)
}

pub fn wait_when_busy(db: &Connection) -> rusqlite::Result<()> {
    db.busy_handler(Some(wait))
}

pub struct Hold {
    deadline: Instant,
}

impl Hold {
    pub fn start(cap: Duration) -> Self {
        let deadline = Instant::now() + cap;
        DEADLINE.set(Some(deadline));
        Self { deadline }
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        DEADLINE.set(None);
    }
}
