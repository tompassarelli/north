//! The mechanism-discovery turn: one issue of its own a UTC day, holding one box on the largest loss in runs, from the
//! first tries lost to the harness and the weekly scoreboard's rows, that no turn of the last 28 days holds, naming the
//! brief that runs the turn (rung three of docs/rationale/self-improvement-loop.md).
use std::collections::BTreeMap;
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
    pub today: String,
    pub losses: Option<Vec<(String, usize)>>,
    pub held: Vec<String>,
    pub turned: Vec<String>,
}

pub fn due(facts: &Facts) -> Vec<Finding> {
    let Some(losses) = facts.losses.as_ref().filter(|_| !facts.turned.contains(&facts.today)) else {
        return Vec::new();
    };
    let mut unheld: Vec<&(String, usize)> = losses.iter().filter(|(key, _)| !facts.held.contains(key)).collect();
    unheld.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let Some((top, _)) = unheld.first() else {
        return Vec::new();
    };
    let named: Vec<String> = unheld.iter().take(NAMED_CLASSES).map(|(key, runs)| format!("{key} ({runs} runs lost)")).collect();
    vec![Finding {
        detector: Detector::MechanismDiscovery,
        key: format!("mechanism-discovery:{}:{top}", facts.today),
        owner: None,
        title: "Run the mechanism-discovery turn".to_owned(),
        detail: format!(
            "Follow {BRIEF} for {today}: take the first key below that no open issue owns, separate its two explanations on the recorded cases, and append the turn to {TURN_LOG}. The largest unheld losses over the last 7 days: {classes}.",
            today = facts.today,
            classes = named.join("; ")
        ),
        done_when: format!("`git show origin/main:{TURN_LOG} | rg -q '^date = {}'` exits 0.", facts.today),
        instances: named,
    }]
}

/// Each `north dash --weekly` row whose k/n figure stands in both weeks and moved against its better
/// direction, keyed `scoreboard:<row>` with the row's own name, worth this week's n times that move in
/// runs. `higher_is_better` is the direction the weekly record holds for the row; a row it does not hold
/// is left out, since a move cannot be called worse without it.
pub fn scoreboard_losses(rows: &str, higher_is_better: &BTreeMap<String, bool>) -> Vec<(String, usize)> {
    rows.lines()
        .filter_map(|line| {
            let (name, columns) = line.strip_prefix("- ")?.split_once(": ")?;
            let (now, before) = columns.split_once(" | ")?;
            let (hits_now, of_now) = share(now)?;
            let (hits_before, of_before) = share(before)?;
            let higher = *higher_is_better.get(name)?;
            let moved = if higher { fraction(hits_before, of_before) - fraction(hits_now, of_now) } else { fraction(hits_now, of_now) - fraction(hits_before, of_before) };
            let lost = (of_now as f64 * moved).round();
            (lost > 0.0).then(|| (format!("scoreboard:{name}"), lost as usize))
        })
        .collect()
}

fn fraction(hits: usize, of: usize) -> f64 {
    if of == 0 { 0.0 } else { hits as f64 / of as f64 }
}

/// The k/n inside a weekly figure, as `north dash --weekly` renders a share (`59% (1180/2000)`, with any
/// trailing note inside the parentheses); a text without one, such as a median's `(n=935)`, holds none.
fn share(text: &str) -> Option<(usize, usize)> {
    let inside = &text[text.find('(')? + 1..];
    let (hits, rest) = inside.split_once('/')?;
    let of: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((hits.parse().ok()?, of.parse().ok()?))
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
