//! The coordinator's relaunch after a launch refusal (north#1577): a refused launch of a task is kept with its refusal's class, the coordinator's next launch of that task counts as a relaunch of that class, and the second of one class within a day is refused unless `--issue` names an open, owned issue that fixes its cause.

use north_unblocks::{Admission, GitHub, Kind, Ledger, Refusals, refusal_class};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::LaunchRequest;
use crate::fleet::runtime::COORDINATOR;

/// A coordinator's launch the relaunch count follows: its key, its session name and when it was asked for.
pub(super) struct Relaunch {
    refusals: Refusals,
    key: String,
    name: String,
    now: u64,
}

impl Relaunch {
    /// Admits the launch `request` that `caller` asks for: only the coordinator's own launch, not one the tick replays from its queue, is followed.
    pub(super) fn admit(caller: Option<&str>, queued: bool, request: &LaunchRequest, issue: Option<&str>, now: u64) -> NorthResult<Option<Self>> {
        if caller != Some(COORDINATOR) || queued {
            return Ok(None);
        }
        let relaunch = Self { refusals: Refusals::here(), key: key(request), name: request.name.clone(), now };
        admit(&relaunch.refusals, &Admission { ledger: &Ledger::here(), issues: &GitHub, now }, &relaunch.key, issue)?;
        Ok(Some(relaunch))
    }

    /// Keeps the refusal of the followed launch `relaunch`, when it was refused.
    pub(super) fn settle(relaunch: Option<&Self>, result: &NorthResult<()>) {
        if let Some(relaunch) = relaunch {
            record(&relaunch.refusals, &relaunch.key, &relaunch.name, result, relaunch.now);
        }
    }
}

/// What a relaunch is counted by: the task and box the launch is for, or its session name when it names no task.
fn key(request: &LaunchRequest) -> String {
    match (&request.task_ref, &request.claim_box) {
        (Some(task), Some(claimed)) => format!("{task} {claimed}"),
        (Some(task), None) => task.clone(),
        (None, _) => request.name.clone(),
    }
}

/// Admits the coordinator's launch of `key`: when its last launch was refused within the day, this launch is a relaunch of that refusal's class.
fn admit(refusals: &Refusals, admission: &Admission, key: &str, issue: Option<&str>) -> NorthResult<()> {
    let Some(class) = refusals.pending(key, admission.now).map_err(NorthError::Refused)? else { return Ok(()) };
    admission.admit(Kind::Relaunch, &class, COORDINATOR, issue).map_err(|why| NorthError::Refused(format!("relaunch of {key} after the refusal `{class}`: {why}")))?;
    refusals.relaunched(key, admission.now).map_err(NorthError::Refused)
}

/// Keeps a refusal of the coordinator's launch of `key`, by its class with the session name folded away.
fn record(refusals: &Refusals, key: &str, name: &str, result: &NorthResult<()>, now: u64) {
    if let Err(NorthError::Refused(why)) = result
        && let Err(failed) = refusals.refused(key, &refusal_class(why, &[name, key]), now)
    {
        eprintln!("fleet launch: the refusal is not kept for the relaunch count: {failed}");
    }
}
