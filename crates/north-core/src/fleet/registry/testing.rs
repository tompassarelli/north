use std::path::PathBuf;
use std::sync::Arc;

use super::{Fleet, FleetBooks, Runtimes};
use crate::error::NorthResult;
use crate::fleet::claims::{Claims, Held};

impl Fleet {
    pub fn at(home: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>) -> NorthResult<Self> {
        Self::at_claiming(home, runtimes, ledger, Arc::new(Held::default()))
    }

    pub fn at_claiming(home: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>) -> NorthResult<Self> {
        let policy = crate::pace::target::home_policy_path(&home);
        Self::rooted(home, policy, runtimes, ledger, claims)
    }

    pub fn hosted_as(mut self, host: &str) -> Self {
        self.host = host.into();
        self
    }
}
