#![cfg(any(test, feature = "test-support"))]

use std::path::{Path, PathBuf};

use crate::error::NorthResult;

use super::model::{Account, Effort, LaunchRequest, SessionRecord};
use super::registry::Fleet;

pub use super::host::alive;
pub use super::remote::placed;
pub use super::transcript::WINDOW_SIZE;

pub trait Internals {
    fn root(&self) -> &Path;
    fn account(&self, request: &LaunchRequest) -> NorthResult<Account>;
    fn record(&self, name: &str) -> NorthResult<SessionRecord>;
    fn session_dir(&self, name: &str) -> NorthResult<PathBuf>;
    fn relaunch_with(&self, name: &str, model: Option<&str>, effort: Option<Effort>, restart: impl FnOnce(&str) -> NorthResult<SessionRecord>) -> NorthResult<SessionRecord>;
    fn refuse_beside_interactive(&self, record: &SessionRecord) -> NorthResult<()>;
}

impl Internals for Fleet {
    fn root(&self) -> &Path {
        &self.root
    }

    fn account(&self, request: &LaunchRequest) -> NorthResult<Account> {
        Fleet::account(self, request)
    }

    fn record(&self, name: &str) -> NorthResult<SessionRecord> {
        Fleet::record(self, name)
    }

    fn session_dir(&self, name: &str) -> NorthResult<PathBuf> {
        Fleet::session_dir(self, name)
    }

    fn relaunch_with(&self, name: &str, model: Option<&str>, effort: Option<Effort>, restart: impl FnOnce(&str) -> NorthResult<SessionRecord>) -> NorthResult<SessionRecord> {
        Fleet::relaunch_with(self, name, model, effort, restart)
    }

    fn refuse_beside_interactive(&self, record: &SessionRecord) -> NorthResult<()> {
        Fleet::refuse_beside_interactive(self, record)
    }
}
