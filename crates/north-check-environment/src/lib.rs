//! Where the lander's candidate checks run (north#782): the repository's own placement (a runner or build host) or this
//! host. A candidate whose base fails in one environment is judged again in the other before any pause is raised, and
//! the environment where a check last reached a verdict is the one new candidates use.
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

/// What one check in one environment showed.
pub enum Reading {
    /// The check reached a verdict about the candidate, so the environment works.
    Reached,
    /// The base tree fails the check there: the environment is broken, not the candidate.
    Broken(String),
    /// No verdict either way.
    Silent,
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

/// Runs `run` in the active environment and, when its base is broken there, in the other one. A verdict reached in
/// either keeps that environment active; a base broken in both returns the last reading's result for the caller to pause on.
pub fn judge<T>(queue: &Path, run: impl FnMut(Environment) -> (T, Reading)) -> T {
    let mut store = Store::host().ok();
    judge_in(store.as_mut(), &instance(queue), run)
}

pub(crate) fn active_in(store: &Store, queue: &str, now: Timestamp) -> Environment {
    let probe = i64::try_from(PROBE.as_millis()).unwrap_or(i64::MAX);
    match store.current::<CheckEnvironment>(queue) {
        Ok(Some(record)) if record.state.is_local() && now.unix_millis() < record.since.unix_millis() + probe => Environment::Local,
        _ => Environment::Placed,
    }
}

pub(crate) fn judge_in<T>(mut store: Option<&mut Store>, queue: &str, mut run: impl FnMut(Environment) -> (T, Reading)) -> T {
    let first = store.as_deref().map_or(Environment::Placed, |store| active_in(store, queue, Timestamp::now()));
    let (result, reading) = run(first);
    let Reading::Broken(detail) = reading else {
        if matches!(reading, Reading::Reached) {
            record(store.as_deref_mut(), queue, first, "a check reached a verdict here");
        }
        return result;
    };
    let second = first.other();
    println!("north land: main's check environment is broken in {} ({detail}); judging the candidate in {} instead", first.name(), second.name());
    let (result, reading) = run(second);
    if matches!(reading, Reading::Reached) {
        record(store, queue, second, &format!("main fails in {}: {detail}", first.name()));
    }
    result
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
