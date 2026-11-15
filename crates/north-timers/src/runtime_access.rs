use north_fleet_supervisor::fleet::model::Account;
use north_fleet_supervisor::fleet::runtime::Invocation;
use north_fleet_supervisor::fleet::runtimes::Runtimes;

/// The provider runtimes an ops job rosters accounts from, and how the provider starts its CLI as one
/// account; the caller that composes the providers supplies both.
pub struct RuntimeAccess {
    pub runtimes: Runtimes,
    pub invocation: fn(&Account) -> Invocation,
}
