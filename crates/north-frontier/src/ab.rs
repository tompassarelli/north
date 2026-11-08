use std::collections::BTreeMap;
use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::report::{minutes, span};
use crate::rows::Arm;
use crate::stats::{bootstrap, bootstrap_two, median};

pub const AB_ROWS: &str = "docs/rationale/frontier/ab.jsonl";
pub const LANDED_PER_ARM: usize = 30;
pub const BEGIN: &str = "<!-- ab-report:begin -->";
pub const END: &str = "<!-- ab-report:end -->";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cost_usd: f64,
}

/// One live A/B box, with the model and effort its sessions ran on (north's route pick, or T3 Code's own default for Claude on the T3 arm); `usage` sums every session the box used, retries and leads included, on both arms. Assigned: assigned by the seeded coin flip to north or T3 Code, with its landing once it lands. Times are Unix seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AbRow {
    #[serde(rename = "box")]
    pub task: String,
    pub arm: Arm,
    pub model: String,
    pub effort: String,
    pub assigned_at: i64,
    #[serde(default)]
    pub landed_sha: Option<String>,
    #[serde(default)]
    pub landed_at: Option<i64>,
    #[serde(default)]
    pub usage: Usage,
}

impl AbRow {
    fn landed_seconds(&self) -> Option<f64> {
        self.landed_sha.as_ref()?;
        Some((self.landed_at? - self.assigned_at) as f64).filter(|seconds| *seconds >= 0.0)
    }
}

pub fn parse(text: &str) -> Result<Vec<AbRow>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let row: AbRow = serde_json::from_str(line).map_err(|why| format!("{AB_ROWS}:{}: {why}", index + 1))?;
            match row.arm {
                Arm::Plain => Err(format!("{AB_ROWS}:{}: the live A/B has two arms, north and t3code, not plain", index + 1)),
                _ => Ok(row),
            }
        })
        .collect()
}

fn landed(rows: &[AbRow], arm: Arm) -> Vec<&AbRow> {
    rows.iter().filter(|row| row.arm == arm && row.landed_seconds().is_some()).collect()
}

pub fn ready(rows: &[AbRow]) -> bool {
    [Arm::North, Arm::T3Code].into_iter().all(|arm| landed(rows, arm).len() >= LANDED_PER_ARM)
}

pub fn progress(rows: &[AbRow]) -> String {
    let counts: Vec<String> = [Arm::North, Arm::T3Code].into_iter().map(|arm| format!("{} {}/{LANDED_PER_ARM}", arm.label(), landed(rows, arm).len())).collect();
    format!("live A/B: landed boxes per arm: {}; the comparison waits for {LANDED_PER_ARM} each\n", counts.join(", "))
}

impl Usage {
    /// Subscription usage as tokens read and written: the ledger marks plan dollars unmeasured, so tokens are what exists.
    fn tokens(&self) -> f64 {
        (self.tokens_in + self.tokens_out) as f64
    }

    pub fn add(&mut self, other: &Usage) {
        self.tokens_in += other.tokens_in;
        self.tokens_out += other.tokens_out;
        self.cost_usd += other.cost_usd;
    }
}

/// The usage of the later sessions whose landed commits changed a box's lines inside its 14 days, keyed by box: a box has an entry once its 14 days have passed.
pub type LongTerm = BTreeMap<String, Usage>;

struct Measure {
    name: &'static str,
    show: fn(f64) -> String,
    delta: fn(f64) -> String,
}

struct Cost {
    look: Measure,
    of: fn(&Usage) -> f64,
}

const COSTS: [Cost; 2] = [
    Cost { look: Measure { name: "subscription usage (tokens in and out)", show: tokens, delta: signed_tokens }, of: Usage::tokens },
    Cost { look: Measure { name: "API-equivalent cost", show: dollars, delta: signed_dollars }, of: |usage| usage.cost_usd },
];

fn tokens(count: f64) -> String {
    format!("{:.0}k tokens", count / 1000.0)
}

fn signed_tokens(count: f64) -> String {
    format!("{:+.0}k tokens", count / 1000.0)
}

fn dollars(amount: f64) -> String {
    format!("${amount:.2}")
}

fn signed_dollars(amount: f64) -> String {
    format!("{amount:+.2} USD")
}

fn median_of(values: &[&f64]) -> Option<f64> {
    median(&values.iter().map(|value| **value).collect::<Vec<_>>())
}

fn write_median_series(out: &mut String, title: &str, measure: &Measure, north: &[f64], t3: &[f64], total: bool) {
    for (label, values) in [("north", north), ("t3code", t3)] {
        let spread = bootstrap(values, median_of);
        let middle = median(values).map_or_else(|| "none".into(), measure.show);
        let sum = if total { format!(", total {}", (measure.show)(values.iter().sum())) } else { String::new() };
        let _ = writeln!(out, "- {title}, {}, {label}: median {middle}{sum} over {} boxes, {}", measure.name, values.len(), span(spread, measure.show));
    }
    let gap = bootstrap_two(north, t3, |first, second| median_of(first).zip(median_of(second)).map(|(a, b)| a - b));
    if let Some(delta) = median(north).zip(median(t3)).map(|(a, b)| a - b) {
        let _ = writeln!(out, "  north minus t3code: median {}, {}", (measure.delta)(delta), span(gap, measure.delta));
    }
}

fn walls(rows: &[&AbRow]) -> Vec<f64> {
    rows.iter().filter_map(|row| row.landed_seconds()).collect()
}

fn usages(rows: &[&AbRow], pick: impl Fn(&AbRow) -> Option<Usage>, of: fn(&Usage) -> f64) -> Vec<f64> {
    rows.iter().filter_map(|row| pick(row)).map(|usage| of(&usage)).collect()
}

pub fn render(rows: &[AbRow], long_term: &LongTerm) -> String {
    let mut out = String::new();
    let (north, t3) = (landed(rows, Arm::North), landed(rows, Arm::T3Code));
    let _ = writeln!(out, "Live A/B on real boxes: north (`north fleet launch`) against T3 Code equipped, each box assigned by a recorded seeded coin flip; {} boxes landed on north, {} on T3 Code. Each interval is a 95% percentile bootstrap within the arm (10,000 resamples, fixed seed); a delta is north minus t3code.", north.len(), t3.len());
    let (north_walls, t3_walls) = (walls(&north), walls(&t3));
    let minutes_measure = Measure { name: "time", show: minutes, delta: minutes };
    write_median_series(&mut out, "assignment to landed", &minutes_measure, &north_walls, &t3_walls, false);
    let _ = writeln!(out, "Short-term usage, summed over every session each box used (retries and leads included):");
    for cost in &COSTS {
        let pick = |row: &AbRow| Some(row.usage.clone());
        write_median_series(&mut out, "per landed box", &cost.look, &usages(&north, pick, cost.of), &usages(&t3, pick, cost.of), true);
    }
    let settled = |group: &[&AbRow]| group.iter().filter(|row| long_term.contains_key(&row.task)).count();
    let _ = writeln!(out, "Long-term usage at 14 days, from the sessions whose landed commits changed the box's lines within 14 days of its landing (by git blame, ledger usage of the task each commit landed); {} of {} north boxes and {} of {} t3code boxes are past 14 days:", settled(&north), north.len(), settled(&t3), t3.len());
    for cost in &COSTS {
        let pick = |row: &AbRow| long_term.get(&row.task).cloned();
        write_median_series(&mut out, "per box", &cost.look, &usages(&north, pick, cost.of), &usages(&t3, pick, cost.of), true);
    }
    let models = |group: &[&AbRow]| {
        let mut seen: Vec<String> = group.iter().map(|row| format!("{} {}", row.model, row.effort)).collect();
        seen.sort();
        seen.dedup();
        seen.join(", ")
    };
    let _ = writeln!(out, "Model and effort: north {}; t3code {}.", models(&north), models(&t3));
    out
}

/// The doc with the report section replaced between its markers, or appended when it has none.
pub fn splice(doc: &str, section: &str) -> String {
    let block = format!("{BEGIN}\n## Live A/B result\n\n{section}{END}\n");
    match (doc.find(BEGIN), doc.find(END)) {
        (Some(start), Some(end)) if start < end => format!("{}{block}{}", &doc[..start], doc[end + END.len()..].trim_start_matches('\n')),
        _ => format!("{}\n{block}", doc.trim_end()),
    }
}
