//! The one-holder rule in front of `north fleet launch --task`: an item an open gate decides is
//! refused until the gate is answered, and an item a running session on any host or a fresh
//! dispatch launch holds is refused, unless `--force-dup REASON` says why it runs beside it.
use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::LaunchRequest;
use crate::fleet::registry::Fleet;
use crate::ops::dispatch::{IssueRef, task_holder};

use super::fleet_list;
use super::fleet_placement::Overrides;

/// The note the session records: placement's, then `dup: REASON` when it runs beside a holder;
/// Err naming the open gate or the holder when nothing says why. `--test` sessions skip the rule.
pub(super) fn noted(fleet: &Fleet, request: &LaunchRequest, overrides: &Overrides, placement: Option<String>) -> NorthResult<Option<String>> {
    let tasks = request.tasks();
    if request.verification || tasks.is_empty() {
        return Ok(placement);
    }
    if let Some(refusal) = gated(&tasks, &fleet.gated_issues()?) {
        return Err(NorthError::Refused(refusal));
    }
    let task = tasks.join(" ");
    let local = Value::Array(fleet.sessions()?.iter().map(|record| fleet_list::session_json(record, fleet.status(record))).collect());
    let mut notes = Vec::new();
    let held = task_holder(&task, local, &mut notes);
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

/// Why a launch naming `tasks` waits: the first task an open gate in `gated` (issue to gate label)
/// decides.
pub(super) fn gated(tasks: &[String], gated: &BTreeMap<String, String>) -> Option<String> {
    tasks.iter().filter_map(|task| IssueRef::parse(task)).find_map(|issue| {
        let gate = gated.get(&issue.to_string())?;
        Some(format!("{issue} waits on open gate {gate}, which decides it; launch work on it after `north gate answer {gate} N` or its default"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_naming_an_issue_an_open_gate_decides_is_refused_with_the_gate_id() {
        let open = BTreeMap::from([("smashcraft#764".to_owned(), "130".to_owned()), ("north#9".to_owned(), "nexus:4".to_owned())]);
        let tasks = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        let refusal = gated(&tasks(&["wisp#3", "tompassarelli/smashcraft#764"]), &open).expect("a gated issue refuses the launch");
        assert!(refusal.starts_with("smashcraft#764 waits on open gate 130"), "{refusal}");
        assert!(gated(&tasks(&["North#9"]), &open).is_some_and(|refusal| refusal.contains("gate nexus:4")), "a relayed host's gate counts");
        assert_eq!(gated(&tasks(&["smashcraft#765", "wisp#3"]), &open), None);
    }
}
