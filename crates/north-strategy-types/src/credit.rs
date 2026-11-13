use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::tokens::TokenUse;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub session: String,
    pub account: Option<String>,
    pub model: Option<String>,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub tokens: TokenUse,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Open {
    pub session: String,
    pub account: String,
    pub model: Option<String>,
    pub tokens: TokenUse,
}

/// What one host knew of its spend at `read_ms`: the runs its ledger holds and the turns running on it then.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub host: String,
    pub read_ms: i64,
    pub finished: Vec<Finished>,
    pub open: Vec<Open>,
}
