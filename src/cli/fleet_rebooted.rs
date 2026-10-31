//! `north fleet rebooted [--revive]`: the sessions this machine's last reboot stranded, and with
//! `--revive` each run again with the turn that says why.
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::rebooted::{Resumer, resume};
use crate::fleet::registry::Fleet;
use crate::platform::host;

use super::fleet_placement::Overrides;
use super::fleet_revive;

const USAGE: &str = "Usage: north fleet rebooted [--revive]";
const REVIVE_BUDGET: Duration = Duration::from_secs(15);

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let revive = match words {
        [] => false,
        ["--revive"] => true,
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    let Some(booted_ms) = host::booted_ms() else { return Err(NorthError::State("cannot read this machine's boot time from /proc/stat".into())) };
    let names = fleet.rebooted(booted_ms)?;
    if !revive {
        names.iter().for_each(|name| println!("{name}"));
        return Ok(());
    }
    let resumed = resume(&Revival(fleet), &names, &host::name(), booted_ms, REVIVE_BUDGET);
    resumed.lines().iter().for_each(|line| println!("{line}"));
    match resumed.failed.len() {
        0 => Ok(()),
        count => {
            resumed.failed.iter().for_each(|(name, why)| eprintln!("not resumed {name}: {why}"));
            Err( NorthError::Refused(format!("north fleet rebooted: {count} sessions not resumed")))
        }
    }
}

struct Revival<'a>(&'a Fleet);

impl Resumer for Revival<'_> {
    fn revive(&self, name: &str) -> NorthResult<()> {
        let mut overrides = Overrides::default();
        if self.0.session(name)?.machine.is_none() {
            overrides.local = Some(format!("north fleet rebooted revives {name} where its conversation lives"));
        }
        fleet_revive::restart(self.0, name, None, None, overrides).map(drop)
    }

    fn send(&self, name: &str, text: &str) -> NorthResult<()> {
        self.0.send_signed(None, name, text, Delivery::Queued)
    }
}
