//! The mechanism-discovery turn: one box a UTC day on the largest first-try loss class that no turn of the last 28 days
//! holds, naming the brief that runs the turn (north#1279, rung three of docs/rationale/self-improvement-loop.md).
use super::{Detector, Finding};

const BRIEF: &str = "harness/mechanism-discovery/turn-brief.md";
const TURN_LOG: &str = "docs/rationale/discovery-turns.toml";
const NAMED_CLASSES: usize = 3;

pub struct Facts {
    pub owner: Option<String>,
    pub today: String,
    pub losses: Vec<(String, usize)>,
    pub held: Vec<String>,
}

pub fn due(facts: &Facts) -> Vec<Finding> {
    let mut unheld: Vec<&(String, usize)> = facts.losses.iter().filter(|(key, _)| !facts.held.contains(key)).collect();
    unheld.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let Some((top, _)) = unheld.first() else {
        return Vec::new();
    };
    let named: Vec<String> = unheld.iter().take(NAMED_CLASSES).map(|(key, runs)| format!("{key} ({runs} first tries lost)")).collect();
    vec![Finding {
        detector: Detector::MechanismDiscovery,
        key: format!("mechanism-discovery:{}:{top}", facts.today),
        owner: facts.owner.clone(),
        title: "Run the mechanism-discovery turn".to_owned(),
        detail: format!(
            "Follow {BRIEF} for {today}: take the first key below that no open issue owns, separate its two explanations on the recorded cases, and append the turn to {TURN_LOG}. The largest unheld classes over the last 7 days: {classes}.",
            today = facts.today,
            classes = named.join("; ")
        ),
        done_when: format!("`git show origin/main:{TURN_LOG} | rg -q '^date = {}'` exits 0.", facts.today),
        instances: named,
    }]
}
