use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

use crate::threads::runs::hook_rules::{Class, OTHER, RULES, find};

pub const PRUNE_DAYS: i64 = 30;
pub const SAMPLES: &str = include_str!("hook_refusal_samples.toml");

#[derive(Debug, Clone, PartialEq)]
pub struct Fire {
    pub session: String,
    pub verdict: String,
    pub rule: String,
    pub at: i64,
    pub turn: i64,
}

#[derive(Debug, Default, PartialEq)]
pub struct RuleStats {
    pub fires: usize,
    pub sessions: usize,
    pub refusals: usize,
    pub rewrites: usize,
    pub advisories: usize,
    pub repeats: usize,
    pub median_lost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub rule: String,
    pub rows: usize,
    pub falses: usize,
    pub narrowed: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Stats {
    pub rules: BTreeMap<String, RuleStats>,
    pub auto_fix: Vec<String>,
    pub skill_text: Vec<String>,
    pub prune: Vec<&'static str>,
}

pub fn samples(text: &str) -> Result<Vec<Sample>, String> {
    let document: toml_edit::DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| format!("hook refusal samples: {error}"))?;
    let tables = document.get("sample").and_then(|item| item.as_array_of_tables()).ok_or("hook refusal samples: no [[sample]] table")?;
    tables
        .iter()
        .map(|table| {
            let rule = table.get("rule").and_then(|item| item.as_str()).ok_or("hook refusal samples: a sample has no rule")?;
            let rows = table.get("row").and_then(|item| item.as_array_of_tables()).ok_or("hook refusal samples: a sample has no rows")?;
            let falses = rows.iter().filter(|row| row.get("label").and_then(|item| item.as_str()) == Some("false")).count();
            let narrowed = table.get("narrowed").and_then(|item| item.as_str()).map(str::to_owned);
            Ok(Sample { rule: rule.to_owned(), rows: rows.len(), falses, narrowed })
        })
        .collect()
}

fn median(mut values: Vec<i64>) -> Option<f64> {
    values.sort_unstable();
    let middle = values.len() / 2;
    match values.len() {
        0 => None,
        count if count % 2 == 1 => Some(values[middle] as f64),
        _ => Some((values[middle - 1] + values[middle]) as f64 / 2.0),
    }
}

fn rule_stats(fires: &[&Fire]) -> RuleStats {
    let count = |verdict: &str| fires.iter().filter(|fire| fire.verdict == verdict).count();
    let mut refused: HashMap<&str, Vec<i64>> = HashMap::new();
    for fire in fires.iter().filter(|fire| fire.verdict == "refusal") {
        refused.entry(fire.session.as_str()).or_default().push(fire.turn);
    }
    let chains: Vec<i64> = refused.values().filter(|turns| turns.len() >= 2).map(|turns| turns.iter().max().unwrap_or(&0) - turns.iter().min().unwrap_or(&0)).collect();
    RuleStats {
        fires: fires.len(),
        sessions: fires.iter().map(|fire| fire.session.as_str()).collect::<BTreeSet<_>>().len(),
        refusals: count("refusal"),
        rewrites: count("rewrite"),
        advisories: count("advisory"),
        repeats: chains.len(),
        median_lost: median(chains),
    }
}

pub fn compute(fires: &[Fire], since: i64, now: i64) -> Stats {
    let mut grouped: BTreeMap<String, Vec<&Fire>> = BTreeMap::new();
    for fire in fires.iter().filter(|fire| fire.at >= since) {
        grouped.entry(fire.rule.clone()).or_default().push(fire);
    }
    let rules: BTreeMap<String, RuleStats> = grouped.into_iter().map(|(rule, fires)| (rule, rule_stats(&fires))).collect();
    let mut auto_fix: Vec<&String> = rules.iter().filter(|(rule, stats)| stats.refusals > 0 && find(rule).is_some_and(|rule| rule.class == Class::Mechanical)).map(|(rule, _)| rule).collect();
    auto_fix.sort_by_key(|rule| std::cmp::Reverse(rules[*rule].refusals));
    let mut skill_text: Vec<&String> = rules.iter().filter(|(rule, stats)| stats.repeats > 0 && find(rule).is_some_and(|rule| !rule.skill.is_empty())).map(|(rule, _)| rule).collect();
    skill_text.sort_by_key(|rule| std::cmp::Reverse(rules[*rule].repeats));
    skill_text.truncate(5);
    let recent: BTreeSet<&str> = fires.iter().filter(|fire| fire.at >= now - PRUNE_DAYS * 86_400).map(|fire| fire.rule.as_str()).collect();
    let prune = RULES.iter().filter(|rule| rule.class != Class::Outside && !recent.contains(rule.id)).map(|rule| rule.id).collect();
    Stats { auto_fix: auto_fix.into_iter().cloned().collect(), skill_text: skill_text.into_iter().cloned().collect(), prune, rules }
}

fn false_share(rule: &str, row: &RuleStats, samples: &[Sample]) -> String {
    if row.refusals == 0 {
        return "-".to_owned();
    }
    samples.iter().rev().find(|sample| sample.rule == rule).map_or_else(
        || "unmeasured".to_owned(),
        |sample| format!("{}/{} ({:.0}%)", sample.falses, sample.rows, 100.0 * sample.falses as f64 / sample.rows.max(1) as f64),
    )
}

pub fn render(stats: &Stats, window: &str, samples: &[Sample]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "hook verdicts, last {window}");
    let _ = writeln!(out, "{:<26} {:>6} {:>8} {:>8} {:>8} {:>8} {:>7} {:>9} {:>14}  class", "rule", "fires", "sessions", "refusals", "rewrites", "advisory", "repeats", "lost(med)", "false share");
    let mut ordered: Vec<(&String, &RuleStats)> = stats.rules.iter().collect();
    ordered.sort_by_key(|(_, stats)| std::cmp::Reverse(stats.fires));
    for (rule, row) in ordered {
        let class = find(rule).map_or(if rule == OTHER { "unclassified" } else { "unknown" }, |rule| rule.class.label());
        let lost = row.median_lost.map_or_else(|| "-".to_owned(), |lost| format!("{lost:.1}"));
        let share = false_share(rule, row, samples);
        let _ = writeln!(out, "{rule:<26} {:>6} {:>8} {:>8} {:>8} {:>8} {:>7} {lost:>9} {share:>14}  {class}", row.fires, row.sessions, row.refusals, row.rewrites, row.advisories, row.repeats);
    }
    let _ = writeln!(out, "\nauto-fix candidates (mechanical refusals still reaching agents):");
    for rule in &stats.auto_fix {
        let row = &stats.rules[rule];
        let share = 100.0 * row.rewrites as f64 / (row.rewrites + row.refusals).max(1) as f64;
        let _ = writeln!(out, "  {rule}: {} refused, {} rewritten ({share:.0}% rewritten)", row.refusals, row.rewrites);
    }
    let _ = writeln!(out, "\nskill-text candidates (refused again in the same session):");
    for rule in &stats.skill_text {
        let row = &stats.rules[rule];
        let _ = writeln!(out, "  {rule}: {} sessions repeated it; line that should have prevented it: {}", row.repeats, find(rule).map_or("", |rule| rule.skill));
    }
    let over: Vec<&Sample> = stats.rules.iter().filter(|(_, row)| row.refusals > 0).filter_map(|(rule, _)| samples.iter().rev().find(|sample| &sample.rule == rule)).filter(|sample| sample.falses * 10 > sample.rows).collect();
    let _ = writeln!(out, "\nover 10% false in the last sample:");
    for sample in &over {
        let _ = writeln!(out, "  {}: {}", sample.rule, sample.narrowed.as_deref().map_or_else(|| "not narrowed".to_owned(), |how| format!("narrowed: {how}")));
    }
    if over.is_empty() {
        let _ = writeln!(out, "  none");
    }
    let _ = writeln!(out, "\nprune candidates (no fire in {PRUNE_DAYS} days):");
    let _ = writeln!(out, "  {}", if stats.prune.is_empty() { "none".to_owned() } else { stats.prune.join(", ") });
    out
}
