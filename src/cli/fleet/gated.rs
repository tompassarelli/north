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
