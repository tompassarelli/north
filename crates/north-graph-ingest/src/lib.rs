//! `north graph ingest`: loads what north already records (the GitHub mirror, the ledger, the push log,
//! the decision log, the corrections ledger and the coverage map) into the fact graph. Each source is
//! read from its local copy, never from GitHub, and told to the graph as a set: a run adds what is
//! new, retracts what a replaced predicate no longer holds, and changes nothing when the sources did not.
mod boxes;
mod coverage;
mod corrections;
mod decisions;
mod facts;
mod issues;
mod ledger;
mod places;
mod pushes;
mod run;
mod shas;

#[cfg(test)]
mod ingest_tests;

pub use places::Places;
pub use run::{Report, Source, ingest, run};
