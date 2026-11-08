//! `north graph ask affected|overlap|blame`: the change queries the lander asks of north's fact graph. A lane's cone
//! is the set of tests its diff can affect, read from the graph's coverage, crate, embed and per-test read facts;
//! `overlap` groups lanes whose cones share a test, and `blame` names the lanes whose cone holds a failure. Depends on
//! north-graph for the facts, north-coverage for hunk carrying and region selection, and north-base for check logs;
//! nothing depends on it but the `north` binary, and the lander reaches it through the command line.
pub mod blame;
pub mod cli;
pub mod cone;
pub mod facts;
pub mod lane;
pub mod overlap;
pub mod workspace;

pub use cli::{VERBS, ask, run};

#[cfg(test)]
mod queries_tests;
