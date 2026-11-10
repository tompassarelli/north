use std::collections::BTreeMap;

use north_base::lifecycle::{Fault, FaultKind};

use super::{Detector, Finding};

const WEEK_MS: i64 = 7 * 24 * 3600 * 1000;

#[derive(Default)]
pub struct Facts {
    pub owner: Option<String>,
    pub faults: Vec<Fault>,
}

/// One box per machine, refused state and week: the store refused a move a writer made, so the table
/// `north check machines` explores and the code that drives it disagree, and no promise of the model says which is
/// right.
pub fn refused(facts: &Facts) -> Vec<Finding> {
    let mut moves: BTreeMap<(&str, &str, i64), Vec<&Fault>> = BTreeMap::new();
    for fault in &facts.faults {
        if let FaultKind::Illegal { to, at_ms } = &fault.kind {
            moves.entry((fault.machine.as_str(), to.as_str(), at_ms / WEEK_MS)).or_default().push(fault);
        }
    }
    moves
        .into_iter()
        .map(|((machine, to, week), faults)| {
            let (count, first) = (faults.len(), faults[0]);
            let was = if first.state.is_empty() { "an instance with no record".to_owned() } else { format!("an instance in {}", first.state) };
            Finding {
                detector: Detector::MachineRefusal,
                key: format!("machine-refusal:{machine}:{to}:{week}"),
                owner: facts.owner.clone(),
                title: format!("Settle {machine}'s refused move to {to}"),
                detail: format!(
                    "The lifecycle store refused {count} move{} of {machine} to {to} in the last hour ({was}, cause \"{}\"), so a writer makes a move the machine's table forbids. Decide which is wrong: let the table allow the move and state the promise in src/cli/machines.rs so `north check machines` holds it, or stop the writer making it.",
                    if count == 1 { "" } else { "s" },
                    first.cause
                ),
                done_when: format!("`north check machines` passes with the decision in the table and its promises, and `north health` reports no illegal {machine} move to {to} for a day after it lands."),
                instances: faults.iter().map(|fault| fault.instance.clone()).collect(),
            }
        })
        .collect()
}
