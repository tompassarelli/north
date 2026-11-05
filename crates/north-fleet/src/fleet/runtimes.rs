use std::path::Path;
use std::sync::Arc;

use crate::error::{NorthError, NorthResult};

use super::model::{Account, LaunchRequest, ProviderName};
use super::roster::Roster;
use super::runtime::Runtime;

#[derive(Clone)]
pub struct Runtimes(Vec<Arc<dyn Runtime>>);

impl Runtimes {
    pub fn new(runtimes: Vec<Arc<dyn Runtime>>) -> Self {
        Self(runtimes)
    }

    pub fn get(&self, provider: &ProviderName) -> NorthResult<Arc<dyn Runtime>> {
        self.0
            .iter()
            .find(|runtime| &runtime.provider() == provider)
            .cloned()
            .ok_or_else(|| NorthError::Usage(format!("no runtime for provider {}", provider.label())))
    }

    pub fn providers(&self) -> Vec<ProviderName> {
        self.0.iter().map(|runtime| runtime.provider()).collect()
    }

    pub fn caller_conversation(&self) -> Option<String> {
        self.0.iter().find_map(|runtime| runtime.caller_conversation())
    }

    pub fn roster(&self, state_root: &Path, home: &Path, policy: &Path) -> Roster {
        self.0.iter().map(|runtime| runtime.roster(state_root, home, policy)).fold(Roster::default(), Roster::merge)
    }

    pub fn account(&self, state_root: &Path, home: &Path, policy: &Path, request: &LaunchRequest) -> NorthResult<Account> {
        match &request.machine {
            Some(machine) => self.get(&request.provider)?.account_on(home, policy, &request.account, &machine.name),
            None => self.roster(state_root, home, policy).account(&request.provider, &request.account),
        }
    }
}
