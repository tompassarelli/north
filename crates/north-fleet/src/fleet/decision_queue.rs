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
        text.push_str(&format!("Decided by default since you last looked ({}): tell the proxy to reverse any.\n", defaulted.len()));
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
    format!(
        "- gate {} ({}, open {hours} h): {}\n  Recommended: {}\n  Alternatives: {}\n  Holds: {holds}\n  Answer: by telling the proxy\n",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::gates::{Closure, GateRequest};

    fn gate(id: i64, question: &str, default: bool, closure: Option<Closure>) -> Gate {
        let request = GateRequest {
            verification: false,
            issue: Some("tompassarelli/north#562".into()),
            box_text: default.then(|| "ledger key".into()),
            session: "n562-w".into(),
            question: question.into(),
            options: vec!["one restricted key per host".into(), "keep local ledgers".into()],
            recommend: 1,
            default: usize::from(default),
            default_after_ms: if default { 8 * 3_600_000 } else { 0 },
        };
        Gate::new(id, request, 0, closure)
    }

    #[test]
    fn the_digest_lists_open_for_tom_gates_with_their_recommendation_and_the_defaults_applied_since_the_last_look() {
        let applied = |closed_ms| Some(Closure { answer: "one restricted key per host".into(), by: Answerer::Default, closed_ms });
        let gates = [
            gate(169, "Give each host a key nexus accepts, which widens access?", false, None),
            gate(170, "Rename the module?", true, None),
            gate(171, "Split the crate?", true, applied(5_000)),
            gate(172, "Merge the docs?", true, applied(500)),
            gate(173, "Give access?", false, Some(Closure { answer: "keep local ledgers".into(), by: Answerer::Tom, closed_ms: 6_000 })),
        ];
        let text = digest(&gates, &[], 1_000, 2 * 3_600_000);
        assert!(text.contains("For Tom (1)") && text.contains("gate 169 (n562-w, open 2 h)") && text.contains("Recommended: one restricted key per host") && text.contains("Alternatives: keep local ledgers") && text.contains("Holds: tompassarelli/north#562"), "{text}");
        assert!(!text.contains("gate 170") && !text.contains("gate 172") && !text.contains("gate 173"), "an ordinary open gate, a default from before the last look and Tom's own answer stay out: {text}");
        assert!(text.contains("Decided by default since you last looked (1)") && text.contains("gate 171"), "{text}");
        assert_eq!(digest(&gates[1..2], &[], 0, 0), "", "nothing for Tom prints nothing, so the session-start hook stays quiet");
    }

    #[test]
    fn a_for_tom_gate_opened_on_another_host_is_in_the_digest() {
        let peer = GateRow::open("gex45", &gate(76, "Give each host a key nexus accepts, which widens access?", false, None));
        let ordinary = GateRow::open("gex45", &gate(77, "Rename the module?", true, None));
        let text = digest(&[], &[peer, ordinary], 0, 2 * 3_600_000);
        assert!(text.contains("For Tom (1)") && text.contains("gate gex45:76 (n562-w, open 2 h)") && text.contains("Recommended: one restricted key per host"), "{text}");
        assert!(!text.contains("gex45:77"), "a peer's gate with a default is not for Tom: {text}");
    }
}
