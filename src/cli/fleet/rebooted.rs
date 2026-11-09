//! `north fleet rebooted [--revive]`: the sessions this machine's last reboot stranded, and with
//! `--revive` each run again with the turn that says why.
use std::sync::Arc;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::rebooted::{Resumer, Revived, resume};
use crate::fleet::registry::Fleet;
use crate::fleet::sender::Sender;
use crate::platform::host;

use super::placement::{Overrides, Refusal};
use super::revive;

const USAGE: &str = "Usage: north fleet rebooted [--revive]";
pub(super) const REVIVE_BUDGET: Duration = Duration::from_secs(15);

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let revive = match words {
        [] => false,
        ["--revive"] => true,
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    if revive {
        return revive_within(fleet, REVIVE_BUDGET);
    }
    fleet.rebooted(booted()?)?.iter().for_each(|name| println!("{name}"));
    Ok(())
}

pub(super) fn revive_within(fleet: &Fleet, budget: Duration) -> NorthResult<()> {
    let booted_ms = booted()?;
    let names = fleet.rebooted(booted_ms)?;
    let resumed = resume(Arc::new(Revival(fleet.clone())), &names, &host::name(), booted_ms, budget);
    resumed.lines().iter().for_each(|line| println!("{line}"));
    match resumed.failed.len() {
        0 => Ok(()),
        count => {
            resumed.failed.iter().for_each(|(name, why)| eprintln!("not resumed {name}: {why}"));
            Err( NorthError::Refused(format!("north fleet rebooted: {count} sessions not resumed")))
        }
    }
}

fn booted() -> NorthResult<u64> {
    host::booted_ms().ok_or_else(|| NorthError::State("cannot read this machine's boot time from /proc/stat".into()))
}

struct Revival(Fleet);

impl Resumer for Revival {
    fn revive(&self, name: &str) -> NorthResult<Revived> {
        let mut overrides = Overrides::default();
        if self.0.session(name)?.machine.is_none() {
            overrides.local = Some(format!("north fleet rebooted revives {name} where its conversation lives"));
        }
        match revive::restart_placed(&self.0, name, None, None, overrides)? {
            Ok(_) => Ok(Revived::Started),
            Err(Refusal::Spend(why)) => Ok(Revived::Waiting(why)),
            Err(refusal) => Err(refusal.into()),
        }
    }

    fn send(&self, name: &str, text: &str) -> NorthResult<()> {
        self.0.send_signed(&Sender::fleet(), name, text, Delivery::Queued)
    }
}
