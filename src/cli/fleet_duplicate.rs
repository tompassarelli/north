//! The one-holder rule in front of `north fleet launch --task`: an item a running session on any host
//! or a fresh dispatch launch holds is refused, unless `--force-dup REASON` says why it runs beside it.
use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::LaunchRequest;
use crate::fleet::registry::Fleet;
use crate::ops::dispatch::task_holder;

use super::fleet_list;
use super::fleet_placement::Overrides;

/// The note the session records: placement's, then `dup: REASON` when it runs beside a holder;
/// Err naming the holder when nothing says why. `--test` sessions skip the rule.
pub(super) fn noted(fleet: &Fleet, request: &LaunchRequest, overrides: &Overrides, placement: Option<String>) -> NorthResult<Option<String>> {
    let Some(task) = request.task_ref.as_deref().filter(|_| !request.verification) else { return Ok(placement) };
    let local = Value::Array(fleet.sessions()?.iter().map(|record| fleet_list::session_json(record, fleet.status(record))).collect());
    let mut notes = Vec::new();
    let held = task_holder(task, local, &mut notes);
    for note in notes {
        eprintln!("fleet launch: {note}");
    }
    let duplicate = match (held, &overrides.duplicate) {
        (None, _) => return Ok(placement),
        (Some(_), Some(reason)) => format!("dup: {reason}"),
        (Some((item, holder)), None) => return Err(NorthError::Refused(format!("{item} is held by {holder}; staff it there, or launch beside it with --force-dup \"<reason>\""))),
    };
    Ok(Some(placement.map_or_else(|| duplicate.clone(), |placement| format!("{placement}; {duplicate}"))))
}
