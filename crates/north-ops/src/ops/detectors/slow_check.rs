use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::{Detector, Finding};

const ROOT: &str = "post-land checks";
const WAITS: [&str; 3] = ["offload sync", "offload slot wait", "capacity lease wait"];

pub const FAMILIES: [(&str, &str); 5] = [("nextest", "cargo nextest run"), ("clippy", "cargo clippy"), ("land_tests", "land-tests"), ("north_build", " --build"), ("head_north", "./bin/head-north")];

pub fn family(command: &str) -> Option<&'static str> {
    FAMILIES.iter().find(|(_, marker)| if marker.starts_with(' ') { command.trim_end().ends_with(marker) } else { command.contains(marker) }).map(|(name, _)| *name)
}

pub fn budget_key(family: &str) -> String {
    format!("land_check_budget_{family}_seconds")
}

pub fn budgets() -> BTreeMap<&'static str, f64> {
    FAMILIES.iter().map(|(family, _)| (*family, super::default_number(&budget_key(family)))).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub family: &'static str,
    pub command: String,
    pub end: f64,
    pub seconds: f64,
}

fn text<'a>(span: &'a Value, key: &str) -> &'a str {
    span.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn number(span: &Value, key: &str) -> f64 {
    span.get(key).and_then(Value::as_f64).unwrap_or_default()
}

pub fn samples(spans: &str) -> Vec<Sample> {
    let spans: Vec<Value> = spans.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let roots: Vec<&str> = spans.iter().filter(|span| text(span, "name") == ROOT).map(|span| text(span, "id")).collect();
    let mut waited: HashMap<&str, f64> = HashMap::new();
    for span in spans.iter().filter(|span| WAITS.contains(&text(span, "name"))) {
        *waited.entry(text(span, "parent")).or_default() += number(span, "end") - number(span, "start");
    }
    let mut found: Vec<Sample> = spans
        .iter()
        .filter(|span| roots.contains(&text(span, "parent")) && span.pointer("/attributes/exit").and_then(Value::as_str) == Some("0"))
        .filter_map(|span| {
            let command = text(span, "name");
            let seconds = number(span, "end") - number(span, "start") - waited.get(text(span, "id")).copied().unwrap_or_default();
            Some(Sample { family: family(command)?, command: command.to_owned(), end: number(span, "end"), seconds: seconds.max(0.0) })
        })
        .collect();
    found.sort_by(|left, right| left.end.total_cmp(&right.end));
    found
}

#[derive(Clone, Debug, PartialEq)]
pub struct Over {
    pub family: &'static str,
    pub budget: f64,
    pub newest: Vec<Sample>,
}

pub fn over(samples: &[Sample], budgets: &BTreeMap<&'static str, f64>, runs: usize) -> Vec<Over> {
    let mut by_family: BTreeMap<&'static str, Vec<&Sample>> = BTreeMap::new();
    for sample in samples {
        by_family.entry(sample.family).or_default().push(sample);
    }
    by_family
        .into_iter()
        .filter_map(|(family, seen)| {
            let budget = *budgets.get(family)?;
            let newest = seen.get(seen.len().checked_sub(runs.max(1))?..)?;
            newest.iter().all(|sample| sample.seconds > budget).then(|| Over { family, budget, newest: newest.iter().map(|sample| (*sample).clone()).collect() })
        })
        .collect()
}

pub fn key(family: &str) -> String {
    format!("slow-check:{family}")
}

pub fn slow(overs: &[Over], owner: &str) -> Vec<Finding> {
    overs
        .iter()
        .map(|over| {
            let times: Vec<String> = over.newest.iter().map(|sample| format!("{:.0} s", sample.seconds)).collect();
            let command = over.newest.last().map(|sample| sample.command.as_str()).unwrap_or_default();
            Finding {
                detector: Detector::SlowCheck,
                key: key(over.family),
                owner: Some(owner.to_owned()),
                title: format!("Bring the {} land check back under its {:.0} s budget", over.family.replace('_', " "), over.budget),
                detail: format!("`{command}` ran {} of its own time (slot and lease waits excluded) in its newest {} green after-landing runs, over the {:.0} s budget {} in harness/defaults.toml", times.join(", "), over.newest.len(), over.budget, budget_key(over.family)),
                done_when: format!("the {} land check runs in at most {:.0} s of its own time in each of {} later green after-landing runs, read from the lander's spans as `north land profile` reads them.", over.family.replace('_', " "), over.budget, over.newest.len()),
                instances: vec![command.to_owned()],
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "slow_check_tests.rs"]
mod tests;
