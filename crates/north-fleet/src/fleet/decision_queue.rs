use crate::fleet::gates::{Answerer, Gate, GateRequest};
use crate::fleet::store::GateRow;

/// What Tom reads when he looks: the for-Tom gates still open on this host and on the peers it has heard from, then the defaults applied here since he last looked.
pub fn digest(gates: &[Gate], elsewhere: &[GateRow], since_ms: u64, now_ms: u64) -> String {
    let here = gates.iter().filter(|gate| gate.is_open() && gate.for_tom()).map(|gate| Waiting { label: gate.id.to_string(), request: &gate.request, opened_ms: gate.opened_ms });
    let there = elsewhere.iter().filter(|gate| gate.request.for_tom()).map(|gate| Waiting { label: gate.label(), request: &gate.request, opened_ms: gate.opened_ms });
    let waiting: Vec<Waiting> = there.chain(here).collect();
    let defaulted: Vec<&Gate> = gates.iter().filter(|gate| gate.closure.as_ref().is_some_and(|closure| closure.by == Answerer::Default && closure.closed_ms >= since_ms)).collect();
    let mut text = String::new();
    if !waiting.is_empty() {
        text.push_str(&format!("For Tom ({}): only the work each holds waits; everything else keeps going.\n", waiting.len()));
        for gate in &waiting {
            text.push_str(&waiting_entry(gate, now_ms));
        }
    }
    if !defaulted.is_empty() {
        text.push_str(&format!("Decided by default since you last looked ({}): tell the coordinator to reverse any.\n", defaulted.len()));
        for gate in defaulted {
            let answer = gate.closure.as_ref().map_or("", |closure| closure.answer.as_str());
            text.push_str(&format!("- gate {} ({}): {}\n  Applied: {answer}\n", gate.id, gate.request.session, one_line(&gate.request.question)));
        }
    }
    text
}

struct Waiting<'a> {
    label: String,
    request: &'a GateRequest,
    opened_ms: u64,
}

fn waiting_entry(gate: &Waiting, now_ms: u64) -> String {
    let hours = now_ms.saturating_sub(gate.opened_ms) / 3_600_000;
    let request = gate.request;
    let option = |number: usize| number.checked_sub(1).and_then(|index| request.options.get(index)).map_or("", String::as_str);
    let alternatives: Vec<&str> = (1..=request.options.len()).filter(|number| *number != request.recommend).map(option).collect();
    let holds = match (&request.issue, &request.box_text) {
        (Some(issue), Some(text)) => format!("the box \"{text}\" of {issue}"),
        (Some(issue), None) => issue.clone(),
        (None, _) => format!("the asking session {}'s question", request.session),
    };
    let category = request.ask_once.as_deref().map_or_else(String::new, |category| format!("  Ask-once category: {category}\n"));
    format!(
        "- gate {} ({}, open {hours} h): {}\n  Recommended: {}\n  Alternatives: {}\n  Holds: {holds}\n{category}  Answer: by telling the coordinator\n",
        gate.label,
        request.session,
        one_line(&request.question),
        option(request.recommend),
        alternatives.join("; ")
    )
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
