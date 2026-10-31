use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::pyfmt::{round_int, round_to};
use crate::ops::advisor_report::stats::float_sum;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RunRow {
    pub scenario: String,
    pub arm: String,
    #[serde(rename = "pass")]
    pub passed: Option<bool>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub wall_s: f64,
    pub model_resolved: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub claude_version: Option<String>,
}

impl RunRow {
    fn tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

pub use crate::platform::trials::Scenarios;

pub fn summary(rows: &[RunRow]) -> String {
    let mut cells: Vec<((&str, &str), Vec<&RunRow>)> = Vec::new();
    for row in rows {
        let key = (row.scenario.as_str(), row.arm.as_str());
        match cells.iter_mut().find(|(known, _)| *known == key) {
            Some((_, members)) => members.push(row),
            None => cells.push((key, vec![row])),
        }
    }
    let mut table = vec![["scenario", "arm", "pass", "err", "tokens in/out/cache", "wall_s"].map(str::to_owned)];
    for ((scenario, arm), members) in &cells {
        let scored: Vec<&&RunRow> = members.iter().filter(|row| row.passed.is_some()).collect();
        let passed = scored.iter().filter(|row| row.passed == Some(true)).count();
        let total = |pick: fn(&RunRow) -> u64| scored.iter().map(|row| pick(row)).sum::<u64>();
        let cache = total(|row| row.cache_creation_tokens) + total(|row| row.cache_read_tokens);
        table.push([
            (*scenario).to_owned(),
            (*arm).to_owned(),
            format!("{passed}/{}", scored.len()),
            (members.len() - scored.len()).to_string(),
            format!("{}/{}/{cache}", total(|row| row.input_tokens), total(|row| row.output_tokens)),
            format!("{:.0}", float_sum(members.iter().map(|row| row.wall_s))),
        ]);
    }
    let widths: Vec<usize> = (0..6).map(|column| table.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    table
        .iter()
        .map(|row| {
            let padded: Vec<String> = row.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect();
            padded.join("  ").trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VariantRow {
    pub variant: String,
    pub policy_sha256: String,
    pub startup_tokens: usize,
    pub conformance: Option<f64>,
    pub n: usize,
    pub scenarios: Scenarios,
    pub errors: usize,
    pub runs_tokens: Option<i64>,
    pub usd_or_null: Option<f64>,
    pub wall_s: f64,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub claude_version: Option<String>,
}

fn common<'a>(values: impl Iterator<Item = Option<&'a str>>) -> Option<String> {
    let mut names: Vec<&str> = values.flatten().filter(|name| !name.is_empty()).collect();
    names.sort_unstable();
    names.dedup();
    (!names.is_empty()).then(|| names.join(","))
}

pub fn variant_row(name: &str, policy: &str, startup: usize, runs: &[RunRow]) -> VariantRow {
    let scored: Vec<&RunRow> = runs.iter().filter(|run| run.passed.is_some()).collect();
    let mut scenarios: Scenarios = BTreeMap::new();
    for run in &scored {
        let cell = scenarios.entry(run.scenario.clone()).or_default();
        cell[0] += usize::from(run.passed == Some(true));
        cell[1] += 1;
    }
    let passed = scored.iter().filter(|run| run.passed == Some(true)).count();
    let tokens: Vec<u64> = scored.iter().map(|run| run.tokens()).collect();
    VariantRow {
        variant: name.to_owned(),
        policy_sha256: format!("{:x}", Sha256::digest(policy.as_bytes())),
        startup_tokens: startup,
        conformance: (!scored.is_empty()).then(|| round_to(passed as f64 / scored.len() as f64, 4)),
        n: scored.len(),
        scenarios,
        errors: runs.len() - scored.len(),
        runs_tokens: (!tokens.is_empty()).then(|| round_int(tokens.iter().sum::<u64>() as f64 / tokens.len() as f64)),
        usd_or_null: None,
        wall_s: round_to(float_sum(runs.iter().map(|run| run.wall_s)), 1),
        model: common(runs.iter().map(|run| run.model_resolved.as_deref().filter(|m| !m.is_empty()).or(run.model.as_deref()))),
        effort: common(runs.iter().map(|run| run.effort.as_deref())),
        claude_version: common(runs.iter().map(|run| run.claude_version.as_deref())),
    }
}
