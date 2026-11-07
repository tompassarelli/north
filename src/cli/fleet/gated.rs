//! The open-gate rule in front of `north fleet launch --task`: an item an open gate decides is
//! refused until the gate is answered.
use crate::dispatch::IssueRef;
use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::Hold;
use crate::fleet::model::LaunchRequest;
use crate::fleet::registry::Fleet;

pub(super) fn admit(fleet: &Fleet, request: &LaunchRequest) -> NorthResult<()> {
    let tasks = request.tasks();
    if request.verification || tasks.is_empty() {
        return Ok(());
    }
    match gated(&tasks, request.item_line().as_deref(), &fleet.gate_holds()?) {
        Some(refusal) => Err(NorthError::Refused(refusal)),
        None => Ok(()),
    }
}

/// Why a launch naming `tasks` with `Item:` line `item` waits: the first task an open gate in `holds` decides, whole or
/// as the one box the line carries.
pub(super) fn gated(tasks: &[String], item: Option<&str>, holds: &[Hold]) -> Option<String> {
    tasks.iter().filter_map(|task| IssueRef::parse(task)).find_map(|issue| {
        let name = issue.to_string();
        let hold = holds.iter().rev().find(|hold| hold.refuses(&name, item))?;
        let gate = &hold.gate;
        Some(format!("{issue} waits on open gate {gate}, which decides it; launch work on it once Tom answers it by telling the proxy, or its default applies"))
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
}
