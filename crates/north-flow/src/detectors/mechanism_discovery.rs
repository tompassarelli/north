//! The mechanism-discovery turn: one box a UTC day on the largest first-try loss class that no turn of the last 28 days
//! holds, naming the brief that runs the turn (north#1279, rung three of docs/rationale/self-improvement-loop.md).
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;

use super::{Detector, Finding};

const BRIEF: &str = "harness/mechanism-discovery/turn-brief.md";
const TURN_LOG: &str = "docs/rationale/discovery-turns.toml";
const NAMED_CLASSES: usize = 3;
const RECORD: &str = "north/mechanism-discovery/turns";
const SCORE: &str = "north/mechanism-discovery/score.txt";

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

pub struct Turn {
    pub date: String,
    pub landed: String,
}

pub struct Mechanism {
    pub verdict: String,
}

pub struct Evidence {
    pub turns: Vec<Turn>,
    pub mechanisms: Vec<Mechanism>,
}

enum Standing {
    Kept,
    Dropped,
    Awaiting,
}

impl Mechanism {
    fn standing(&self) -> Standing {
        self.verdict
            .lines()
            .find_map(|line| {
                let kept = line.starts_with("- [x] Keep") || (line.starts_with("- [x] Settle") && line.contains(" on keep")) || (line.contains("Evidence:") && line.contains("passed on"));
                let dropped = line.starts_with("- [ ] Revert") || line.contains("missed on");
                if kept { Some(Standing::Kept) } else if dropped { Some(Standing::Dropped) } else { None }
            })
            .unwrap_or(Standing::Awaiting)
    }
}

pub fn score(evidence: &Evidence) -> String {
    let newest = evidence.turns.iter().map(|turn| turn.date.as_str()).max().unwrap_or("none");
    let count = |wanted: fn(&Standing) -> bool| evidence.mechanisms.iter().filter(|mechanism| wanted(&mechanism.standing())).count();
    format!(
        "turns {} in 28 d, newest {newest}; mechanisms registered {}, kept on held-out cases {}, dropped {}, awaiting review {}",
        evidence.turns.len(),
        evidence.mechanisms.len(),
        count(|standing| matches!(standing, Standing::Kept)),
        count(|standing| matches!(standing, Standing::Dropped)),
        count(|standing| matches!(standing, Standing::Awaiting)),
    )
}

/// Writes one record file per turn not yet recorded and the score line; with no turns the record is left untouched, so `north loops check` reads it stale.
pub fn write(state: &Path, evidence: &Evidence) -> Result<(), String> {
    let record = state.join(RECORD);
    for turn in &evidence.turns {
        let path = record.join(format!("{}.txt", turn.date));
        let made = fs::create_dir_all(&record).and_then(|()| fs::OpenOptions::new().write(true).create_new(true).open(&path));
        match made {
            Ok(mut file) => file.write_all(format!("turn {} landed {}\n", turn.date, turn.landed).as_bytes()).map_err(|why| format!("{}: {why}", path.display()))?,
            Err(why) if why.kind() == ErrorKind::AlreadyExists => {}
            Err(why) => return Err(format!("{}: {why}", path.display())),
        }
    }
    let path = state.join(SCORE);
    fs::create_dir_all(path.parent().unwrap_or(state)).and_then(|()| fs::write(&path, format!("{}\n", score(evidence)))).map_err(|why| format!("{}: {why}", path.display()))
}
