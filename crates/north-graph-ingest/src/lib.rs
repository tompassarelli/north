//! `north graph ingest`: loads what north already records (the GitHub mirror, the ledger, the push log,
//! the corrections ledger, the coverage map, the workspace manifests and the tests' recorded reads) into the fact graph. Each source is read from its
//! local copy, never from GitHub, and told to the graph as a set: a run adds what is
//! new, retracts what a replaced predicate no longer holds, and changes nothing when the sources did not.
mod boxes;
mod coverage;
mod corrections;
mod crates;
mod facts;
mod issues;
mod ledger;
mod losses;
mod observations;
mod places;
mod pushes;
mod ranking;
mod reads;
mod run;
mod shas;
mod source;
mod touches;
mod trusted_writers;
mod waits;

#[cfg(test)]
mod ingest_tests;
#[cfg(test)]
mod waits_tests;

pub use places::Places;
pub use run::{Report, ingest, run};
pub use source::Source;
