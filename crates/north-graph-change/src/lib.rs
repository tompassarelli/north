//! `north graph ask affected|overlap|blame`: the change queries the lander asks of north's fact graph. A lane's cone
//! is the set of tests its diff can affect, read from the graph's coverage, crate, embed and per-test read facts;
//! `overlap` groups lanes whose cones share a test, and `blame` names the lanes whose cone holds a failure.
//! `affected` also computes phase 2 of docs/rationale/test-selection.md, the item-reference proof, and logs it beside
//! the gating selection while `[gate.test-selection-proof]` is in shadow. Depends on north-graph for the facts,
//! north-coverage for hunk carrying and region selection, and north-base for check logs; nothing depends on it but
//! the `north` binary, and the lander reaches it through the command line.
//! `north graph split` turns a task and a base commit into boxes whose parts share no file, reading only the base
//! tree and one planning call through the `split::Planner` port its caller fills.
pub mod apart;
pub mod blame;
pub mod carry;
pub mod cli;
pub mod cone;
pub mod facts;
pub mod lane;
pub mod overlap;
pub mod shadow;
pub mod split;
pub mod workspace;

pub use cli::{VERBS, ask, run};

#[cfg(test)]
mod queries_tests;
