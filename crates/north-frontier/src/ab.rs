use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::report::{minutes, span};
use crate::rows::Arm;
use crate::stats::{Interval, bootstrap, bootstrap_two, mean, median, wilson};

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

/// What went wrong with a landing in the 48 hours after it: filled from git and the ledger once the window has closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rework {
    pub reverts: u32,
    pub red_main: u32,
    pub reopened: u32,
}

impl Rework {
    fn happened(self) -> bool {
        self.reverts + self.red_main + self.reopened > 0
    }
}

/// The landed diff's effect on the code's health.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeHealth {
    pub net_lines: i64,
    pub new_findings: u32,
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
    #[serde(default)]
    pub interventions: u32,
    #[serde(default)]
    pub rework: Option<Rework>,
    #[serde(default)]
    pub code_health: Option<CodeHealth>,
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

fn seconds(rows: &[&AbRow]) -> Vec<f64> {
    rows.iter().filter_map(|row| row.landed_seconds()).collect()
}

pub fn ready(rows: &[AbRow]) -> bool {
    [Arm::North, Arm::T3Code].into_iter().all(|arm| landed(rows, arm).len() >= LANDED_PER_ARM)
}

pub fn progress(rows: &[AbRow]) -> String {
    let counts: Vec<String> = [Arm::North, Arm::T3Code].into_iter().map(|arm| format!("{} {}/{LANDED_PER_ARM}", arm.label(), landed(rows, arm).len())).collect();
    format!("live A/B: landed boxes per arm: {}; the comparison waits for {LANDED_PER_ARM} each\n", counts.join(", "))
}

fn median_wall(walls: &[&f64]) -> Option<f64> {
    median(&walls.iter().map(|wall| **wall).collect::<Vec<_>>())
}

fn median_of(rows: &[&AbRow], value: impl Fn(&AbRow) -> Option<f64>) -> Option<f64> {
    median(&rows.iter().filter_map(|row| value(row)).collect::<Vec<_>>())
}

fn gap_of(north: &[&AbRow], t3: &[&AbRow], value: impl Fn(&AbRow) -> Option<f64>) -> Option<f64> {
    Some(median_of(north, &value)? - median_of(t3, &value)?)
}

fn mean_of(rows: &[&AbRow], value: impl Fn(&AbRow) -> Option<f64>) -> Option<f64> {
    mean(&rows.iter().filter_map(|row| value(row)).collect::<Vec<_>>())
}

fn interval_clear_below(interval: Option<Interval>) -> bool {
    interval.is_some_and(|found| found.high < 0.0)
}

fn interval_clear_above(interval: Option<Interval>) -> bool {
    interval.is_some_and(|found| found.low > 0.0)
}

fn rework_flag(row: &AbRow) -> Option<f64> {
    row.rework.map(|rework| f64::from(u8::from(rework.happened())))
}

fn findings(row: &AbRow) -> Option<f64> {
    row.code_health.map(|health| f64::from(health.new_findings))
}

fn net_lines(row: &AbRow) -> Option<f64> {
    row.code_health.map(|health| health.net_lines as f64)
}

fn dollars(amount: f64) -> String {
    format!("${amount:.2}")
}

fn signed_dollars(amount: f64) -> String {
    format!("{amount:+.2} USD")
}

fn points(share: f64) -> String {
    format!("{:+.0} points", share * 100.0)
}

pub fn render(rows: &[AbRow]) -> String {
    let mut out = String::new();
    let (north, t3) = (landed(rows, Arm::North), landed(rows, Arm::T3Code));
    let _ = writeln!(out, "Live A/B on real boxes: north (`north fleet launch`) against T3 Code equipped, each box assigned by a recorded seeded coin flip; {} boxes landed on north, {} on T3 Code.", north.len(), t3.len());
    let north_walls = seconds(&north);
    let t3_walls = seconds(&t3);
    for (label, walls) in [("north", &north_walls), ("t3code", &t3_walls)] {
        let middle = median(walls).map_or_else(|| "none landed".into(), minutes);
        let spread = bootstrap(walls, |sample| median(&sample.iter().map(|wall| **wall).collect::<Vec<_>>()));
        let _ = writeln!(out, "- assignment to landed, {label}: median {middle} over {} boxes, {}", walls.len(), span(spread, minutes));
    }
    let speed = bootstrap_two(&north_walls, &t3_walls, |first, second| median_wall(first).zip(median_wall(second)).map(|(a, b)| a - b));
    if let Some(delta) = median(&north_walls).zip(median(&t3_walls)).map(|(a, b)| a - b) {
        let _ = writeln!(out, "  north minus t3code: median {}, {}", minutes(delta), span(speed, minutes));
    }
    let (north_all, t3_all): (Vec<AbRow>, Vec<AbRow>) = (north.iter().map(|row| (*row).clone()).collect(), t3.iter().map(|row| (*row).clone()).collect());
    let (north_refs, t3_refs): (Vec<&AbRow>, Vec<&AbRow>) = (north_all.iter().collect(), t3_all.iter().collect());
    let mut guards_hold = true;
    let cost = |row: &AbRow| Some(row.usage.cost_usd);
    for (label, group) in [("north", &north_refs), ("t3code", &t3_refs)] {
        let spread = bootstrap(group, |sample| median(&sample.iter().map(|row| row.usage.cost_usd).collect::<Vec<_>>()));
        let _ = writeln!(out, "- API-equivalent cost per landed box, {label}: median {} over {} boxes, {}", dollars(median_of(group, cost).unwrap_or_default()), group.len(), span(spread, dollars));
    }
    let price = bootstrap_two(&north_all, &t3_all, |first, second| gap_of(first, second, cost));
    if let Some(delta) = median_of(&north_refs, cost).zip(median_of(&t3_refs, cost)).map(|(a, b)| a - b) {
        let _ = writeln!(out, "  north minus t3code: median {}, {}", signed_dollars(delta), span(price, signed_dollars));
    }
    let interventions = |row: &AbRow| Some(f64::from(row.interventions));
    let line: Vec<String> = [("north", &north_refs), ("t3code", &t3_refs)].into_iter().map(|(name, group)| format!("{name} {:.2}", mean_of(group, interventions).unwrap_or_default())).collect();
    let _ = writeln!(out, "- interventions per box: {}", line.join(", "));
    let models = |group: &[&AbRow]| {
        let mut seen: Vec<String> = group.iter().map(|row| format!("{} {}", row.model, row.effort)).collect();
        seen.sort();
        seen.dedup();
        seen.join(", ")
    };
    let _ = writeln!(out, "- model and effort: north {}; t3code {}", models(&north_refs), models(&t3_refs));
    let _ = writeln!(out, "Maintainability guards (north must not be worse on either):");
    let known = |group: &[&AbRow]| group.iter().filter(|row| row.rework.is_some()).count();
    for (label, group) in [("north", &north_refs), ("t3code", &t3_refs)] {
        let counted = known(group);
        let hits = group.iter().filter(|row| row.rework.is_some_and(Rework::happened)).count();
        let share = if counted == 0 { "no landing past its 48 h yet".into() } else { format!("{hits}/{counted}, {}", span(wilson(hits, counted), |value| format!("{:.0}%", value * 100.0))) };
        let _ = writeln!(out, "- rework within 48 h (revert, red main or reopened box), {label}: {share}");
    }
    let rework_gap = bootstrap_two(&north_all.iter().filter(|row| row.rework.is_some()).cloned().collect::<Vec<_>>(), &t3_all.iter().filter(|row| row.rework.is_some()).cloned().collect::<Vec<_>>(), |first, second| mean_of(first, rework_flag).zip(mean_of(second, rework_flag)).map(|(a, b)| a - b));
    if let Some(interval) = rework_gap {
        let _ = writeln!(out, "  north minus t3code: {}", span(Some(interval), points));
    }
    guards_hold &= !interval_clear_above(rework_gap);
    for (label, group) in [("north", &north_refs), ("t3code", &t3_refs)] {
        let _ = writeln!(out, "- code health per landed diff, {label}: median net lines {:+.0}, mean new `north code-health` findings {:.2} over {} diffs", median_of(group, net_lines).unwrap_or_default(), mean_of(group, findings).unwrap_or_default(), group.iter().filter(|row| row.code_health.is_some()).count());
    }
    let finding_gap = bootstrap_two(&north_all, &t3_all, |first, second| mean_of(first, findings).zip(mean_of(second, findings)).map(|(a, b)| a - b));
    if let Some(interval) = finding_gap {
        let _ = writeln!(out, "  north minus t3code, new findings per diff: {}", span(Some(interval), |value| format!("{value:+.2}")));
    }
    let lines_gap = bootstrap_two(&north_all, &t3_all, |first, second| gap_of(first, second, net_lines));
    if let Some(interval) = lines_gap {
        let _ = writeln!(out, "  north minus t3code, median net lines per diff: {}", span(Some(interval), |value| format!("{value:+.0} lines")));
    }
    guards_hold &= !interval_clear_above(finding_gap) && !interval_clear_above(lines_gap);
    let (faster, slower) = (interval_clear_below(speed), interval_clear_above(speed));
    let (cheaper, dearer) = (interval_clear_below(price), interval_clear_above(price));
    let verdict = if !guards_hold {
        "a guard is clearly worse for north: no win until it is explained."
    } else if (cheaper && !slower) || (faster && !dearer) {
        "north is clearly cheaper and not clearly slower, or clearly faster and not clearly dearer, with neither guard clearly worse: north wins."
    } else if slower || dearer {
        "north is clearly slower or dearer without the offsetting win: north does not win."
    } else {
        "neither the speed nor the cost difference is clear at 95%: no winner."
    };
    let _ = writeln!(out, "Verdict by the rule (a gap is clear when its 95% interval excludes zero; north wins when it is clearly cheaper and not clearly slower, or clearly faster and not clearly dearer, and no guard gap is clearly worse): {verdict}");
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
