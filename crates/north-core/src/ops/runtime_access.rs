use crate::fleet::model::Account;
use crate::fleet::runtime::Invocation;
use crate::fleet::runtimes::Runtimes;

/// The provider runtimes an ops job rosters accounts from, and how the provider starts its CLI as one
/// account; the caller that composes the providers supplies both.
pub struct RuntimeAccess {
    pub runtimes: Runtimes,
    pub invocation: fn(&Account) -> Invocation,
}
