//! `north fleet revive`: restart a stopped session, re-running placement for a Claude box.
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord};
use crate::fleet::registry::{Fleet, REVIVED_MID_TURN};

use super::placement::{self, Launch, Overrides, Refusal};

const USAGE: &str = "Usage: north fleet revive NAME [--system-prompt FILE] [--effort low|medium|high|xhigh|max] [--local REASON] [--pin REASON] [--account LABEL]";

pub(super) fn revive_with(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let (mut system_prompt, mut effort, mut overrides, mut account) = (None, None, Overrides::default(), None);
    for pair in words.chunks(2) {
        match pair {
            ["--system-prompt", file] => system_prompt = Some(std::fs::canonicalize(file)?),
            ["--effort", level] => effort = Some(Effort::parse(level)?),
            ["--account", label] => account = Some(*label),
            [flag, value] if overrides.set(flag, value) => {}
            _ => return Err(NorthError::Usage(USAGE.into())),
        }
    }
    restart_onto(fleet, name, system_prompt, effort, overrides, account).map(|record| println!("{}", record.name))
}

/// Revive re-runs placement for a stopped Claude session, keeping the reasons its launch recorded and
/// the machine its conversation lives on.
pub(super) fn revive(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, effort: Option<Effort>, overrides: Overrides) -> NorthResult<()> {
    restart(fleet, name, system_prompt, effort, overrides).map(|record| println!("{}", record.name))
}

pub(super) fn restart(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, effort: Option<Effort>, overrides: Overrides) -> NorthResult<SessionRecord> {
    restart_onto(fleet, name, system_prompt, effort, overrides, None)
}

/// Restart, on `account` when it names another one, carrying the conversation there.
pub(super) fn restart_onto(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, effort: Option<Effort>, overrides: Overrides, account: Option<&str>) -> NorthResult<SessionRecord> {
    let cut_short = if fleet.session(name).is_ok() { fleet.left_mid_turn(name)? } else { false };
    let record = placed_onto(fleet, name, system_prompt, effort, overrides, account)??;
    if cut_short {
        fleet.carry_on(name, REVIVED_MID_TURN)?;
    }
    Ok(record)
}

/// Restart, keeping placement's refusal apart from the errors of the restart itself.
pub(super) fn restart_placed(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, effort: Option<Effort>, overrides: Overrides) -> NorthResult<Result<SessionRecord, Refusal>> {
    placed_onto(fleet, name, system_prompt, effort, overrides, None)
}

fn placed_onto(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, effort: Option<Effort>, overrides: Overrides, account: Option<&str>) -> NorthResult<Result<SessionRecord, Refusal>> {
    let record = fleet.revivable(name)?;
    if let (Some(machine), Some(_)) = (&record.machine, &overrides.local) {
        return Err(NorthError::Usage(format!("{name}'s conversation lives on {}, so it revives there; launch a new session to work here", machine.name)));
    }
    let mut placement = None;
    let moving = account.is_some_and(|to| to != record.account);
    if record.provider.label() == "claude" && !record.verification && (moving || !fleet.running(&record)) {
        let overrides = overrides.recorded(record.placement.as_deref());
        let brief = system_prompt.clone().or_else(|| record.system_prompt.clone());
        let model = record.model.as_deref();
        let launch = Launch {
            name,
            account: account.unwrap_or(&record.account),
            brief: brief.as_deref(),
            cwd: None,
            machine: record.machine.as_ref().map(|machine| machine.name.as_str()),
            lead: false,
            role: record.role(),
            model,
            family: model.and_then(crate::usage::meter::family).unwrap_or(crate::usage::decide::world::ModelFamily::Opus),
            repository: &crate::pacing::placement::gate::launch_repository(Some(&record.cwd), record.task_ref.as_deref()),
        };
        placement = match placement::check(&launch, &placement::brief_text(brief.as_deref(), "")?, &overrides, &placement::machine_load(fleet)?, &fleet.roster(), true)? {
            Ok(placed) => placed.note,
            Err(refusal) => return Ok(Err(refusal)),
        };
    }
    fleet.revive_placed(name, system_prompt, placement, effort, account).map(Ok)
}
