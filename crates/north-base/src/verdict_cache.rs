mod key;
mod store;
#[cfg(test)]
mod tests;

pub use key::{Check, HEAD_NORTH_DEFERRED, NORTH_IGNORES, NORTH_READS, built_equivalent, key, machine_bound, runtime_identity};
pub use store::{Pass, Store, remote_hits, remote_record, reused_line};
