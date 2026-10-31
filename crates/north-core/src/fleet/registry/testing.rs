use std::path::PathBuf;
use std::sync::Arc;

use super::{Fleet, FleetBooks, Runtimes};
use crate::error::NorthResult;
use crate::fleet::claims::{Claims, Held};
use crate::fleet::store::{Claim, Registrar, SessionRow};

pub struct Unrecorded;

impl Registrar for Unrecorded {
    fn register(&self, _: &SessionRow, _: Claim) -> NorthResult<()> {
        Ok(())
    }
}

impl Fleet {
    pub fn at(home: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>) -> NorthResult<Self> {
        Self::at_claiming(home, runtimes, ledger, Arc::new(Held::default()))
    }

    pub fn at_claiming(home: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>) -> NorthResult<Self> {
        let policy = crate::pace::target::home_policy_path(&home);
        Self::rooted(home, policy, runtimes, ledger, claims, Arc::new(Unrecorded))
    }

    pub fn arming(mut self, arm: fn(i64) -> NorthResult<()>) -> Self {
        self.arm = arm;
        self
    }

    pub fn hosted_as(mut self, host: &str) -> Self {
        self.host = host.into();
        self
    }

    pub fn registering(mut self, registrar: Arc<dyn Registrar>) -> Self {
        self.registrar = registrar;
        self
    }
}
