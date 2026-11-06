//! The lander's judgements replayed, and the landing tail they leave.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use north_base::platform::decision_log;
use north_strategy_types::land::{JudgeCase, Judgement};
use north_strategy_types::{Envelope, Kind};
use serde::Deserialize;

use crate::Deciders;
use crate::bounds::Bounds;
use crate::report::{Counts, Invariant, stamp};

#[derive(Deserialize)]
struct Stamp {
    id: String,
    at: f64,
}

fn label(judgement: &Judgement) -> String {
    match serde_json::to_value(judgement) {
        Ok(serde_json::Value::String(name)) => name,
        Ok(serde_json::Value::Object(fields)) => fields.keys().next().cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

fn percentile(sorted: &[f64], share: f64) -> f64 {
    let rank = ((sorted.len() as f64 * share).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// Replays every judgement in the window. A ticket's landing time is the check time of the rounds that
/// carried it until the replayed judgement lands it, with the idle gaps between rounds left out; a ticket
/// the log landed and the replay never does counts as never landing.
pub fn replay(root: &Path, since: f64, now: f64, deciders: &Deciders, bounds: &Bounds) -> (Counts, Invariant) {
    let batches: HashMap<String, f64> = decision_log::lines(root, Kind::LandBatch, "jsonl", since - 3600.0, now)
        .into_iter()
        .filter_map(|(_, line)| serde_json::from_str::<Stamp>(&line).ok())
        .map(|stamp| (stamp.id, stamp.at))
        .collect();
    let mut judged: Vec<Envelope<JudgeCase, Judgement>> = decision_log::lines(root, Kind::LandJudge, "jsonl", since, now)
        .into_iter()
        .filter_map(|(_, line)| serde_json::from_str(&line).ok())
        .filter(|logged: &Envelope<JudgeCase, Judgement>| logged.at >= since)
        .collect();
    judged.sort_by(|a, b| a.at.total_cmp(&b.at));
    let mut counts = Counts::new("land.judge");
    let mut spent: HashMap<&str, f64> = HashMap::new();
    let mut stuck: BTreeSet<&str> = BTreeSet::new();
    let mut closed: BTreeSet<&str> = BTreeSet::new();
    let mut times: Vec<(f64, f64, &str)> = Vec::new();
    for logged in &judged {
        let replayed = (deciders.judge)(&logged.case);
        counts.record(&label(&logged.decision), Some(&label(&replayed)));
        let wall = logged.follows.as_ref().and_then(|batch| batches.get(batch)).map_or(0.0, |started| logged.at - started);
        for branch in logged.case.included.iter().map(String::as_str) {
            *spent.entry(branch).or_default() += wall;
            let logged_land = logged.decision == Judgement::Land;
            match (logged_land, replayed == Judgement::Land) {
                (_, true) if closed.contains(branch) => {
                    spent.remove(branch);
                    if logged_land {
                        closed.remove(branch);
                    }
                }
                (_, true) => {
                    stuck.remove(branch);
                    times.push((spent.remove(branch).unwrap_or_default(), logged.at, branch));
                    if !logged_land {
                        closed.insert(branch);
                    }
                }
                (true, false) => {
                    stuck.insert(branch);
                }
                (false, false) => {}
            }
        }
    }
    (counts, tail(times, &stuck, bounds))
}

fn tail(mut times: Vec<(f64, f64, &str)>, stuck: &BTreeSet<&str>, bounds: &Bounds) -> Invariant {
    let bound = bounds.landing_p90_seconds;
    let mut tail = Invariant::new("landing-p90", format!("replayed tickets land within {bound} s of check time at the 90th percentile"));
    times.sort_by(|a, b| a.0.total_cmp(&b.0));
    let samples: Vec<f64> = times.iter().map(|(seconds, _, _)| *seconds).chain(stuck.iter().map(|_| f64::INFINITY)).collect();
    let mut sorted = samples.clone();
    sorted.sort_by(f64::total_cmp);
    tail.checked = samples.len();
    tail.passed = samples.iter().filter(|seconds| **seconds <= bound).count();
    if sorted.is_empty() {
        return tail;
    }
    let p90 = percentile(&sorted, 0.9);
    if p90 > bound {
        let worst = times.last().map_or_else(String::new, |(seconds, at, branch)| format!("; slowest landed {branch} took {seconds:.0} s, landed at {}", stamp(*at)));
        let never: Vec<&&str> = stuck.iter().take(3).collect();
        tail.breaches.push(format!("p90 is {p90:.1} s over {bound} s{worst}; never landed under replay: {} {never:?}", stuck.len()));
    }
    tail
}
