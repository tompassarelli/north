//! The ledger image peers read: the ledger host builds one from SQLite, streams the pages a caller's
//! manifest lacks, and the caller publishes the rebuilt snapshot beside its own ledger. `serve` is the
//! host's side, `pull` the caller's schedule, `receive` the page stream, `image` the build, and `pages`
//! the wire format and page hashes both sides share.
mod image;
mod pages;
mod pull;
mod receive;
mod serve;
mod sidecar;

pub use image::normalize;
pub use pages::{MAGIC, manifest_bytes};
pub use pull::{last_failure, pull, refresh, take_turn};
pub use serve::{REMOTE_COMMAND, serve};

use std::time::Duration;

pub const SNAPSHOT_SECONDS: u64 = 900;
/// How old a snapshot may be before a call that reads it pulls it again.
pub const SNAPSHOT_AGE: Duration = Duration::from_secs(SNAPSHOT_SECONDS);
pub const PULL_TIMEOUT: Duration = Duration::from_secs(80);
/// How soon a host with no snapshot yet tries again after a pull that could not finish, which resumes from its partial file.
pub const FIRST_PULL_RETRY: Duration = Duration::from_secs(60);
