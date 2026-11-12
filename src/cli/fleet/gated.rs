//! The open-gate rule in front of `north fleet launch --task`: an item an open gate decides is
//! refused until the gate is answered.
use crate::dispatch::IssueRef;
use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{Gate, Hold};
use crate::fleet::model::LaunchRequest;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;

const ANSWERS_KEPT_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Refuses a launch an open gate decides, else appends to its prompt the answer of each gate that decided its issue box and closed with no session left to read it.
pub(super) fn admit(fleet: &Fleet, request: &mut LaunchRequest) -> NorthResult<()> {
    let tasks = request.tasks();
    if request.verification || tasks.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = [request.item_line(), request.claimed_box()].into_iter().flatten().collect();
    let item = (!lines.is_empty()).then(|| lines.join("\n"));
    if let Some(refusal) = gated(&tasks, item.as_deref(), &fleet.gate_holds()?) {
        return Err(NorthError::Refused(refusal));
    }
    let unread = fleet.unread_answers(now_ms().saturating_sub(ANSWERS_KEPT_MS))?;
    if let Some(section) = decided(&tasks, item.as_deref(), &unread) {
        request.prompt.push_str(&section);
    }
    Ok(())
}

/// The prompt section carrying the recorded answer of each gate in `closed` that decides a task of the launch or the box its `Item:` line names.
pub(super) fn decided(tasks: &[String], item: Option<&str>, closed: &[Gate]) -> Option<String> {
    let decides = |gate: &Gate| {
        let Some(issue) = gate.request.issue.clone() else { return false };
        let hold = Hold { issue, box_text: gate.request.box_text.clone(), gate: gate.id.to_string() };
        tasks.iter().filter_map(|task| IssueRef::parse(task)).any(|task| hold.refuses(&task.to_string(), item))
    };
    let answers: Vec<String> = closed
        .iter()
        .filter(|gate| decides(gate))
        .filter_map(|gate| {
            let closure = gate.closure.as_ref()?;
            Some(format!("- gate {}: {} Answer ({}): {}", gate.id, gate.request.question.replace('\n', " "), closure.by.label(), closure.answer.replace('\n', " / ")))
        })
        .collect();
    (!answers.is_empty()).then(|| format!("\n\nDecided before this launch, by gates whose asking session retired before it read the answer:\n{}\n", answers.join("\n")))
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

    #[test]
    fn a_launch_on_an_issue_box_carries_the_answer_of_a_gate_that_decided_it_for_a_retired_asker() {
        use crate::fleet::gates::{Answerer, Closure, GateRequest};
        let request = GateRequest { verification: false, issue: Some("north#735".into()), box_text: Some("gate belongs".into()), session: "n-cx53".into(), question: "Which owner?".into(), options: vec!["a".into(), "b".into()], recommend: 1, default: 0, default_after_ms: 0, ask_once: None };
        let closed = [Gate::new(180, request, 1, Some(Closure { answer: "b\nship it".into(), by: Answerer::Tom, closed_ms: 2 }))];

        let section = decided(&tasks(&["north#735"]), Some(" north#735 Box: A gate belongs to the issue box"), &closed).expect("the box carries the answer");

        assert!(section.contains("gate 180: Which owner? Answer (tom): b / ship it"), "{section}");
        assert_eq!(decided(&tasks(&["north#735"]), Some(" north#735 Box: another box"), &closed), None);
    }
}
