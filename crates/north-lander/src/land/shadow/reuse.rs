use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::slots::unit_reuse::harvest;
use super::super::slots::unit_reuse::lines::Record;
use super::super::slots::unit_reuse::{self, USAGE as OBSERVE_USAGE};
use super::super::trace::report::duration_seconds;

pub const USAGE: &str = "north land reuse report [--since DURATION] (prints the landings and units whose per-test lookups the lander logged, the share that would have been reused by count and by test-seconds, and each disagreement) | observe | available (the host side of the nextest land check)";
const DEFAULT_SINCE: &str = "1h";

#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub landings: usize,
    pub units: usize,
    pub reused: usize,
    pub seconds: f64,
    pub reused_seconds: f64,
    pub disagreements: Vec<String>,
}

pub fn summarize(records: &[Record]) -> Summary {
    let mut summary = Summary::default();
    let mut probes = BTreeSet::new();
    for record in records {
        match record {
            Record::Unit { probe, unit, set, passed, seconds } => {
                probes.insert(probe.as_str());
                summary.units += 1;
                summary.seconds += seconds;
                if let Some(set) = set {
                    summary.reused += 1;
                    summary.reused_seconds += seconds;
                    if !passed {
                        summary.disagreements.push(format!("{unit} in {probe}: path set {} would have carried a pass and the real run failed", short(set)));
                    }
                }
            }
            Record::SetDiffers { probe, unit, reused, real, path } => {
                probes.insert(probe.as_str());
                summary.disagreements.push(format!("{unit} in {probe}: every answer matched path set {} and the real run read set {}; first difference: {path}", short(reused), short(real)));
            }
        }
    }
    summary.landings = probes.len();
    summary
}

fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

fn share(part: f64, whole: f64) -> f64 {
    if whole > 0.0 { 100.0 * part / whole } else { 0.0 }
}

#[allow(clippy::cast_precision_loss)]
pub fn render(summary: &Summary, since: &str) -> String {
    let mut text = format!(
        "north land reuse report: the last {since}\nlandings: {}\nunits: {}\nwould reuse: {} of {} units ({:.1}%)\nwould reuse by test-seconds: {:.1} of {:.1} s ({:.1}%)\ndisagreements: {}\n",
        summary.landings,
        summary.units,
        summary.reused,
        summary.units,
        share(summary.reused as f64, summary.units as f64),
        summary.reused_seconds,
        summary.seconds,
        share(summary.reused_seconds, summary.seconds),
        summary.disagreements.len()
    );
    for disagreement in &summary.disagreements {
        text.push_str(&format!("  disagreement: {disagreement}\n"));
    }
    text
}

fn report(arguments: &[String]) -> u8 {
    let since = match arguments {
        [] => DEFAULT_SINCE,
        [flag, value] if flag == "--since" => value,
        _ => {
            eprintln!("usage: {USAGE}");
            return 64;
        }
    };
    let Some(seconds) = duration_seconds(since) else {
        eprintln!("north land reuse report: {since:?} is not a duration such as 30m, 1h or 2d");
        return 64;
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let from = now.saturating_sub(seconds as u64);
    let records: Vec<Record> = harvest::read_since(&harvest::directory(), from).into_iter().map(|(_, record)| record).collect();
    print!("{}", render(&summarize(&records), since));
    0
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "report" => report(rest),
        Some((verb, _)) if verb == "observe" || verb == "available" => unit_reuse::run(arguments),
        _ => {
            eprintln!("usage: {USAGE}\n       {OBSERVE_USAGE}");
            64
        }
    }
}
