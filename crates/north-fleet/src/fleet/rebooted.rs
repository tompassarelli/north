//! Sessions a host reboot stranded: the machine went down under a session that was neither stopped
//! nor finished, so its host process is gone and nothing runs it again. `north fleet rebooted
//! --revive` runs each one again with the turn that tells it why, from every fleet tick on the host.
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::NorthResult;
use crate::fleet::stage::{Late, detach};
use crate::foundation::clock::Timestamp;

/// How a rebooted host's sessions are run again and told so.
pub trait Resumer: Send + Sync {
    fn revive(&self, name: &str) -> NorthResult<Revived>;
    fn send(&self, name: &str, text: &str) -> NorthResult<()>;
}

/// The turn a revived session gets: why it ran again and where to pick up.
pub fn message(host: &str, booted_ms: u64) -> String {
    let at = Timestamp::from_unix_millis(i64::try_from(booted_ms).unwrap_or(i64::MAX));
    format!("host {host} rebooted at {}; resume from your last checkpoint", at.iso_minute())
}

/// A revive that started the session, or one its account's spent window holds until it resets.
#[derive(Debug, Eq, PartialEq)]
pub enum Revived {
    Started,
    Waiting(String),
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Resumed {
    pub resumed: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// Sessions whose account is out, which a later run revives once its window resets.
    pub waiting: Vec<(String, String)>,
    /// Sessions the time budget left for the next run.
    pub deferred: Vec<String>,
}

impl Resumed {
    pub fn lines(&self) -> Vec<String> {
        let resumed = self.resumed.iter().map(|name| format!("resumed {name}"));
        let failed = self.failed.iter().map(|(name, why)| format!("not resumed {name}: {why}"));
        let waiting = self.waiting.iter().map(|(name, why)| format!("waiting {name}: {why}"));
        let deferred = self.deferred.iter().map(|name| format!("deferred {name} to the next run"));
        resumed.chain(failed).chain(waiting).chain(deferred).collect()
    }
}

/// Revives and messages each of `names`, in order, starting no new one once `budget` is spent and
/// waiting on each only until that point: a timer unit that outlives its timeout is killed between
/// a revive and its message.
pub fn resume(resumer: Arc<dyn Resumer>, names: &[String], host: &str, booted_ms: u64, budget: Duration) -> Resumed {
    let text = message(host, booted_ms);
    let started = Instant::now();
    let mut outcome = Resumed::default();
    for name in names {
        let left = budget.saturating_sub(started.elapsed());
        if left.is_zero() {
            outcome.deferred.push(name.clone());
            continue;
        }
        let resumer = resumer.clone();
        let (named, told) = (name.clone(), text.clone());
        let revived = detach("rebooted", move || revive_and_tell(resumer.as_ref(), &named, &told)).by(Instant::now() + left);
        match revived {
            Ok(Ok(Revived::Started)) => outcome.resumed.push(name.clone()),
            Ok(Ok(Revived::Waiting(why))) => outcome.waiting.push((name.clone(), why)),
            Ok(Err(error)) => outcome.failed.push((name.clone(), error.user_message())),
            Err(Late::Running) => outcome.deferred.push(name.clone()),
            Err(Late::Panicked) => outcome.failed.push((name.clone(), "it panicked".to_owned())),
            Err(Late::Unstarted(error)) => outcome.failed.push((name.clone(), format!("its thread did not start: {error}"))),
        }
    }
    outcome
}

fn revive_and_tell(resumer: &dyn Resumer, name: &str, text: &str) -> NorthResult<Revived> {
    resumer.revive(name).and_then(|revived| match revived {
        Revived::Started => resumer.send(name, text).map(|()| Revived::Started),
        waiting => Ok(waiting),
    })
}

#[cfg(test)]
#[path = "rebooted_tests.rs"]
mod tests;
