//! Sessions a host reboot stranded: the machine went down under a session that was neither stopped
//! nor finished, so its host process is gone and nothing runs it again. `north fleet rebooted
//! --revive` runs each one again with the turn that tells it why, from the fleet-relay timer once
//! the host answers ssh.
use std::time::{Duration, Instant};

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

/// How a rebooted host's sessions are run again and told so.
pub trait Resumer {
    fn revive(&self, name: &str) -> NorthResult<()>;
    fn send(&self, name: &str, text: &str) -> NorthResult<()>;
}

/// The turn a revived session gets: why it ran again and where to pick up.
pub fn message(host: &str, booted_ms: u64) -> String {
    let at = Timestamp::from_unix_millis(i64::try_from(booted_ms).unwrap_or(i64::MAX));
    format!("host {host} rebooted at {}; resume from your last checkpoint", at.iso_minute())
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Resumed {
    pub resumed: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// Sessions the time budget left for the next run.
    pub deferred: Vec<String>,
}

impl Resumed {
    pub fn lines(&self) -> Vec<String> {
        let resumed = self.resumed.iter().map(|name| format!("resumed {name}"));
        let failed = self.failed.iter().map(|(name, why)| format!("not resumed {name}: {why}"));
        let deferred = self.deferred.iter().map(|name| format!("deferred {name} to the next run"));
        resumed.chain(failed).chain(deferred).collect()
    }
}

/// Revives and messages each of `names`, in order, starting no new one once `budget` is spent: a
/// timer unit that outlives its timeout is killed between a revive and its message.
pub fn resume(resumer: &dyn Resumer, names: &[String], host: &str, booted_ms: u64, budget: Duration) -> Resumed {
    let text = message(host, booted_ms);
    let started = Instant::now();
    let mut outcome = Resumed::default();
    for name in names {
        if started.elapsed() >= budget {
            outcome.deferred.push(name.clone());
            continue;
        }
        match resumer.revive(name).and_then(|()| resumer.send(name, &text)) {
            Ok(()) => outcome.resumed.push(name.clone()),
            Err(error) => outcome.failed.push((name.clone(), error.user_message())),
        }
    }
    outcome
}

#[cfg(test)]
#[path = "rebooted_tests.rs"]
mod tests;
