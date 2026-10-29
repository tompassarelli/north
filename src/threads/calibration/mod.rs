//! The NorthThreads calibration core: which tier to recommend for a brief, the walk-forward ETA and
//! $ it replays from strictly earlier runs, and the box chains the box ETA is measured on. Every
//! tunable is a field of [`Knobs`], so a sweep can run the same core under other values.
mod boxes;
mod calibrate;
mod forward;
mod mode;
mod read;
mod recommend;
mod replay;
#[cfg(test)]
mod tests;
mod verbs;

use std::sync::LazyLock;

use toml_edit::DocumentMut;

use crate::threads::runs::brief::CATEGORIES;

pub(crate) use recommend::Pick;
pub(crate) use verbs::{VERBS, run};
pub(crate) use boxes::{box_chains, box_duration, moment};
pub(crate) use forward::walk_forward_eta;
pub(crate) use read::{Raw, box_rows, calibration_runs, escalated, raw_runs};
pub(crate) use recommend::{recommend, recommend_usd};

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

const TIER_ORDER: [&str; 10] = [
    "claude:haiku", "codex:luna-low", "claude:sonnet-high", "codex:sol-medium", "codex:sol-high", "codex:sol-xhigh",
    "claude:opus-medium", "claude:opus-high", "claude:opus-xhigh", "codex:astra",
];
const RUN_TIERS: [(&str, &str); 10] = [
    ("claude:haiku", "haiku"),
    ("claude:sonnet-high", "sonnet"),
    ("claude:opus-medium", "medium"),
    ("claude:opus-high", "high"),
    ("claude:opus-xhigh", "xhigh"),
    ("codex:luna-low", "gpt-6-luna low"),
    ("codex:sol-medium", "gpt-6.1-sol medium"),
    ("codex:sol-high", "gpt-6.1-sol high"),
    ("codex:sol-xhigh", "gpt-6.1-sol xhigh"),
    ("codex:astra", "gpt-6-astra xhigh"),
];

fn default_tier() -> String {
    DEFAULTS["default_tier"]["value"].as_str().expect("default_tier is text").to_owned()
}

/// The brief facets a cell is made of, category first, as the runs table holds them.
pub(crate) const GROUPS: [&str; 5] = ["category", "spec", "scope", "surface", "verify"];
/// Facets in the order a cell gives them up when it holds too few runs, first given up last.
const BACKOFF: [&str; 4] = ["surface", "verify", "scope", "spec"];
const PROVIDERS: [&str; 4] = ["claude", "codex", "gemini", "all"];

fn registry(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

/// The registry values the calibration core reads; `Knobs::registry` gives the production ones.
#[derive(Clone, Debug)]
pub(crate) struct Knobs {
    pub success_threshold: f64,
    pub min_runs: usize,
    pub incumbent_margin: i64,
    pub usd_min: usize,
    pub eta_min_samples: usize,
    pub eta_coverage: f64,
    pub box_chain_gap_min: f64,
    pub box_calibration_n: usize,
    pub calibration_thin_n: usize,
    pub calibration_stale_days: f64,
}

impl Knobs {
    pub(crate) fn registry() -> Self {
        let min_runs = registry("min_runs") as usize;
        Self {
            success_threshold: registry("success_threshold"),
            min_runs,
            incumbent_margin: registry("incumbent_margin") as i64,
            usd_min: min_runs,
            eta_min_samples: registry("eta_min_samples") as usize,
            eta_coverage: registry("eta_coverage"),
            box_chain_gap_min: registry("box_chain_gap_min"),
            box_calibration_n: registry("box_calibration_n") as usize,
            calibration_thin_n: registry("calibration_thin_n") as usize,
            calibration_stale_days: registry("calibration_stale_days"),
        }
    }
}

/// One worker run as the core reads it. `cells` holds the GROUPS values, `-` for a facet the brief left out.
#[derive(Clone, Debug, Default)]
pub(crate) struct Run {
    pub cells: [String; 5],
    pub tier: String,
    pub minutes: Option<f64>,
    pub eta_min: Option<f64>,
    pub cost_usd: Option<f64>,
    pub outcome: String,
    pub ended: String,
    pub first: bool,
    pub ok: bool,
    pub unjudged: bool,
}

impl Run {
    pub(crate) fn category(&self) -> &str {
        &self.cells[0]
    }
}

/// The tier key a run's tier column names: a RUN_TIERS value maps back to its key, anything else is kept.
pub(crate) fn tier_key(tier: &str) -> String {
    let tier = match tier.trim() {
        "" => "-",
        trimmed => trimmed,
    };
    if RUN_TIERS.iter().any(|(key, _)| *key == tier) {
        return tier.to_owned();
    }
    RUN_TIERS.iter().find(|(_, run)| *run == tier).map_or(tier, |(key, _)| key).to_owned()
}

pub(crate) fn tier_rank(tier: &str) -> usize {
    let key = tier_key(tier);
    TIER_ORDER.iter().position(|ordered| *ordered == key).unwrap_or(TIER_ORDER.len())
}

fn provider(tier: &str) -> String {
    let key = tier_key(tier);
    if let Some((provider, _)) = key.split_once(':') {
        provider.to_owned()
    } else if key.starts_with("gpt-") {
        "codex".to_owned()
    } else if key.starts_with("gemini") {
        "gemini".to_owned()
    } else {
        "claude".to_owned()
    }
}

/// A brief value for a cell: a category outside CATEGORIES is `unknown`, an empty facet `-`.
pub(crate) fn cell_value(key: &str, value: Option<&str>) -> String {
    let value = value.filter(|value| !value.is_empty());
    if key == "category" {
        return value.filter(|value| CATEGORIES.contains(value)).unwrap_or("unknown").to_owned();
    }
    value.unwrap_or("-").to_owned()
}

/// The element at ceil(len * fraction) - 1 of the sorted values, None for no values.
pub(crate) fn quantile(values: &[f64], fraction: f64) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    sorted.get(index).copied()
}

/// The lower middle value, None for no values.
pub(crate) fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len().checked_sub(1)? / 2).copied()
}

/// `raw` times the calibration scale, rounded up to whole minutes when `whole`.
pub(crate) fn scaled(raw: f64, scale: Option<f64>, whole: bool) -> f64 {
    let value = scale.filter(|scale| *scale != 0.0).map_or(raw, |scale| raw * scale);
    if whole { value.ceil() } else { value }
}
