//! `north graph`: Beagle Store's triple kernel and five verbs (tell, retract, show, ask, validate) as
//! north's fact graph. Terms are interned to integer ids in SQLite, which holds rows, indexes,
//! durability and concurrent writers; `ask` runs a Datalog engine written in Rust over them.
mod cli;
mod json;
mod key;
mod query;
mod store;
mod term;
mod text;

#[cfg(test)]
mod kernel_tests;
#[cfg(test)]
mod store_tests;

pub use cli::run;
pub use error::{GraphError, QueryError};
pub use json::{term_from_json, term_to_json};
pub use query::{Answer, Budget, DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS, Query, STEP_BUDGET, parse_query};
pub use store::{Action, ActionKind, Graph, Live, Receipt, Report, default_path};
pub use term::{Atom, Float, Instant, Term, TermError, Triple};
pub use text::parse_arg;

mod error;
