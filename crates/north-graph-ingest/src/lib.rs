//! `north graph ingest`: loads what north already records (the coverage map, the workspace manifests and the tests' recorded reads) into the fact graph. Each source is read from its
//! local copy, never from GitHub, and told to the graph as a set: a run adds what is
//! new, retracts what a replaced predicate no longer holds, and changes nothing when the sources did not.
mod coverage;
mod crates;
mod facts;
mod places;
mod reads;
mod run;
mod source;
mod trusted_writers;

#[cfg(test)]
mod ingest_tests;

pub use places::Places;
pub use run::{Report, ingest, run};
pub use source::Source;
