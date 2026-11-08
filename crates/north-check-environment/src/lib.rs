//! Where the lander's checks run (north#782): the repository's own placement (a runner or build host) or this host,
//! as the lander doctor last settled it.
pub mod credential;

use std::path::Path;
use std::time::Duration;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Lifecycle, State, Store};

/// How long a fallback to this host holds before the placement is tried again.
const PROBE: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    /// The repository's own placement: its runner or build host.
    Placed,
    /// This host, under its capacity lease and without the runner.
    Local,
}

impl Environment {
    pub fn other(self) -> Self {
        match self {
            Self::Placed => Self::Local,
            Self::Local => Self::Placed,
        }
    }

    pub fn is_local(self) -> bool {
        self == Self::Local
    }
}

impl State for Environment {
    const ALL: &'static [Self] = &[Self::Placed, Self::Local];

    fn name(self) -> &'static str {
        match self {
            Self::Placed => "placed",
            Self::Local => "local",
        }
    }
}

/// One queue's check environment; the instance is the queue's directory name. Each verdict reached on this host moves
/// the record again, so its `since` is the last landing that depended on the fallback.
pub struct CheckEnvironment;

impl Lifecycle for CheckEnvironment {
    const NAME: &'static str = "check-environment";
    type State = Environment;

    fn legal(_: Option<Environment>, _: Environment) -> bool {
        true
    }

    fn dwell(_: Environment) -> Option<Duration> {
        None
    }

    fn orphan(_: Environment) -> Option<Environment> {
        None
    }
}

/// The lifecycle instance name of a queue: its directory name.
pub fn instance(queue: &Path) -> String {
    queue.file_name().map_or_else(|| queue.display().to_string(), |name| name.to_string_lossy().into_owned())
}

pub fn active(queue: &Path) -> Environment {
    Store::host().ok().map_or(Environment::Placed, |store| active_in(&store, &instance(queue), Timestamp::now()))
}

/// Makes `environment` the one new candidates use, as a verdict reached there would.
pub fn settle(queue: &Path, environment: Environment, cause: &str) {
    if let Ok(mut store) = Store::host() {
        record(Some(&mut store), &instance(queue), environment, cause);
    }
}

pub(crate) fn active_in(store: &Store, queue: &str, now: Timestamp) -> Environment {
    let probe = i64::try_from(PROBE.as_millis()).unwrap_or(i64::MAX);
    match store.current::<CheckEnvironment>(queue) {
        Ok(Some(record)) if record.state.is_local() && now.unix_millis() < record.since.unix_millis() + probe => Environment::Local,
        _ => Environment::Placed,
    }
}

fn record(store: Option<&mut Store>, queue: &str, environment: Environment, cause: &str) {
    let Some(store) = store else { return };
    let recorded = store.current::<CheckEnvironment>(queue).ok().flatten().map(|record| record.state);
    if environment.is_local() || recorded.is_some_and(Environment::is_local) {
        let _ = store.transition::<CheckEnvironment>(queue, environment, cause, Timestamp::now());
    }
}

#[cfg(test)]
mod tests;
