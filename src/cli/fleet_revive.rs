//! `north fleet revive`: restart a stopped session, re-running placement for a Claude box.
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord};
use crate::fleet::registry::Fleet;

use super::fleet_placement::{self, Launch, Overrides};

const USAGE: &str = "Usage: north fleet revive NAME [--system-prompt FILE] [--effort low|medium|high|xhigh|max] [--local REASON] [--pin REASON]";

pub(super) fn revive_with(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let (mut system_prompt, mut effort, mut overrides) = (None, None, Overrides::default());
    for pair in words.chunks(2) {
        match pair {
            ["--system-prompt", file] => system_prompt = Some(std::fs::canonicalize(file)?),
            ["--effort", level] => effort = Some(Effort::parse(level)?),
            [flag, value] if overrides.set(flag, value) => {}
            _ => return Err(NorthError::Usage(USAGE.into())),
        }
    }
    revive(fleet, name, system_prompt, effort, overrides)
}

/// Revive re-runs placement for a stopped Claude session, keeping the reasons its launch recorded and
/// the machine its conversation lives on.
pub(super) fn revive(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, effort: Option<Effort>, overrides: Overrides) -> NorthResult<()> {
    restart(fleet, name, system_prompt, effort, overrides).map(|record| println!("{}", record.name))
}

pub(super) fn restart(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, effort: Option<Effort>, overrides: Overrides) -> NorthResult<SessionRecord> {
    let record = fleet.session(name)?;
    if let (Some(machine), Some(_)) = (&record.machine, &overrides.local) {
        return Err(NorthError::Usage(format!("{name}'s conversation lives on {}, so it revives there; launch a new session to work here", machine.name)));
    }
    let mut placement = None;
    if record.provider.label() == "claude" && !record.verification && !fleet.running(&record) {
        let overrides = overrides.recorded(record.placement.as_deref());
        let brief = system_prompt.clone().or_else(|| record.system_prompt.clone());
        let launch = Launch { name, account: &record.account, brief: brief.as_deref(), cwd: None, machine: record.machine.as_ref().map(|machine| machine.name.as_str()) };
        placement = fleet_placement::check(&launch, &fleet_placement::brief_text(brief.as_deref(), "")?, &overrides, &fleet_placement::machine_load(fleet)?, &fleet.roster(), true)??.note;
    }
    fleet.revive_placed(name, system_prompt, placement, effort)
}
