//! The one-holder rule in front of `north fleet launch --task`: an item an open gate decides is
//! refused until the gate is answered, and an item a running session on any host or a fresh
//! dispatch launch holds is refused, unless `--force-dup REASON` says why it runs beside it.
use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::Hold;
use crate::fleet::model::LaunchRequest;
use crate::fleet::registry::Fleet;
use crate::dispatch::{IssueRef, task_holder};

use super::list;
use super::placement::Overrides;

/// The note the session records: placement's, then `dup: REASON` when it runs beside a holder;
/// Err naming the open gate or the holder when nothing says why. `--test` sessions skip the rule.
pub(super) fn noted(fleet: &Fleet, request: &LaunchRequest, overrides: &Overrides, placement: Option<String>) -> NorthResult<Option<String>> {
    let tasks = request.tasks();
    if request.verification || tasks.is_empty() {
        return Ok(placement);
    }
    if let Some(refusal) = gated(&tasks, request.item_line().as_deref(), &fleet.gate_holds()?) {
        return Err(NorthError::Refused(refusal));
    }
    let task = tasks.join(" ");
    let local = Value::Array(fleet.sessions()?.iter().map(|record| list::session_json(record, fleet.status(record), &fleet.waits())).collect());
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

/// Why a launch naming `tasks` with `Item:` line `item` waits: the first task an open gate in `holds` decides, whole or
/// as the one box the line carries.
pub(super) fn gated(tasks: &[String], item: Option<&str>, holds: &[Hold]) -> Option<String> {
    tasks.iter().filter_map(|task| IssueRef::parse(task)).find_map(|issue| {
        let name = issue.to_string();
        let hold = holds.iter().rev().find(|hold| hold.refuses(&name, item))?;
        let gate = &hold.gate;
        Some(format!("{issue} waits on open gate {gate}, which decides it; launch work on it after `north gate answer {gate} N` or its default"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(issue: &str, box_text: Option<&str>, gate: &str) -> Hold {
        Hold { issue: issue.to_owned(), box_text: box_text.map(str::to_owned), gate: gate.to_owned() }
    }

    fn tasks(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_launch_naming_an_issue_an_open_gate_decides_is_refused_with_the_gate_id() {
        let open = [hold("makgora#764", None, "130"), hold("north#9", None, "nexus:4")];
        let refusal = gated(&tasks(&["wisp#3", "tompassarelli/makgora#764"]), None, &open).expect("a gated issue refuses the launch");
        assert!(refusal.starts_with("makgora#764 waits on open gate 130"), "{refusal}");
        assert!(gated(&tasks(&["North#9"]), Some(" north#9 any box"), &open).is_some_and(|refusal| refusal.contains("gate nexus:4")), "a relayed host's gate counts, and a whole-issue gate holds every box");
        assert_eq!(gated(&tasks(&["makgora#765", "wisp#3"]), None, &open), None);
    }

    #[test]
    fn a_gate_scoped_to_one_box_holds_only_the_launch_naming_that_box() {
        let open = [hold("north#88", Some("gh-app-token"), "151")];
        let other = " north#88 oauth refresh port";
        let gated_box = " north#88 Port gh-app-token to Rust";
        assert_eq!(gated(&tasks(&["north#88"]), Some(other), &open), None, "another box of the umbrella is admitted");
        let refusal = gated(&tasks(&["north#88"]), Some(gated_box), &open).expect("the gated box is refused");
        assert!(refusal.starts_with("north#88 waits on open gate 151"), "{refusal}");
        assert!(gated(&tasks(&["north#88"]), None, &open).is_some(), "a launch naming no box cannot be told apart, so it waits");
        assert_eq!(gated(&tasks(&["north#89"]), Some(gated_box), &open), None, "another issue is untouched");
    }
}
