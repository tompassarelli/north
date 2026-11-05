//! The decision records north's kernel writes and its strategies read: one `Case` and one `Decision`
//! per decision kind, and the envelope every line of the decision log carries
//! (docs/rationale/strategy-kernel.md).
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod admission;
pub mod envelope;
pub mod land;
pub mod placement;

pub use envelope::{Envelope, Kind, Snapshot, VERSION};
