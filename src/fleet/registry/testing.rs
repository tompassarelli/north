use std::path::PathBuf;
use std::sync::Arc;

use super::{Fleet, FleetBooks, Runtimes};
use crate::error::NorthResult;

impl Fleet {
    pub fn at(home: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>) -> NorthResult<Self> {
        let policy = crate::pace::target::home_policy_path(&home);
        Self::rooted(home, policy, runtimes, ledger)
    }
}
