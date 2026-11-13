//! The step after a DeepSeek worker reports `Done:` (north#1020). Dispatch queues one `north fleet dispatch review` job on
//! the host that holds the session, through the laptop job queue or here, as it queues launches, also for a session the
//! fleet retired before any job named it (north#1231). The job reads the lane's commit where the lane lives, over ssh on
//! the build host a session placed there ran on, and by `sampling` either launches a Sonnet reviewer on the lane and hands
//! the lane to it, or forgets the session and lands the lane through safe-push. A reviewer records its verdict with
//! `north fleet dispatch verdict`, which sampling and the DeepSeek overlay read.
//!
//! `done` finds the sessions that reported Done, `queue` queues their jobs, `lane` reads the lane each worked in and
//! `brief` tells the reviewer what it rejects and who lands; `serve` runs one job and `verdict` records a review.

mod brief;
mod done;
mod lane;
mod queue;
mod serve;
mod verdict;

pub use brief::{brief, worker_order, Lander, CLASSES};
pub use done::{done, has_waiting, queued, Done, Found, Marked};
pub use queue::{job, queue};
pub use serve::serve;
pub use verdict::verdict;
#[cfg(test)]
pub(super) use serve::no_commit_kind;
