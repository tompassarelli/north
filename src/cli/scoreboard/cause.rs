//! `north scoreboard rework`: the week's reworked landings by class of change and by what the rework fixed,
//! so the fixes can target the class that reworks most.
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{Sources, Window, open};
use crate::threads::calibration::rework::{Cause, Judged, Rework};

const EXAMPLES: usize = 3;
const SUBJECT_CHARS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Code,
    Gates,
    Tests,
    Tables,
    Docs,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Self::Code => "control-plane code",
            Self::Gates => "gates, guards and hooks",
            Self::Tests => "tests",
            Self::Tables => "tables",
            Self::Docs => "docs",
        }
    }

    fn of_file(path: &str) -> Self {
        let has = |parts: &[&str]| parts.iter().any(|part| path.contains(part));
        if has(&["_tests.rs", "/tests/", "/tests.rs", ".test.sh", ".test.ts"]) {
            Self::Tests
        } else if path.starts_with("docs/") || path.ends_with(".md") {
            Self::Docs
        } else if [".toml", ".json", ".yaml", ".lock"].iter().any(|suffix| path.ends_with(suffix)) {
            Self::Tables
        } else if has(&["guard", "hook", "safe-push", "denylist", "land/", "landing", "gate"]) {
            Self::Gates
        } else {
            Self::Code
        }
    }

    /// The class most of a landing's files fall in, the earlier class winning a tie.
    fn of_landing(files: &[String]) -> Self {
        let mut counts: BTreeMap<Self, usize> = BTreeMap::new();
        for file in files {
            *counts.entry(Self::of_file(file)).or_default() += 1;
        }
        counts.into_iter().max_by_key(|(class, count)| (*count, std::cmp::Reverse(*class))).map_or(Self::Code, |(class, _)| class)
    }
}

#[derive(Default)]
struct Tally {
    landings: usize,
    reverted: usize,
    fixed_forward: usize,
    culprit: usize,
    fixes: Vec<(i64, String)>,
}

impl Tally {
    fn reworked(&self) -> usize {
        self.reverted + self.fixed_forward + self.culprit
    }
}

/// Each landing once, reworked if any run that landed it was.
fn landings<'a>(judged: impl Iterator<Item = &'a Judged>) -> Vec<&'a Judged> {
    let mut by_sha: BTreeMap<&str, &Judged> = BTreeMap::new();
    for entry in judged {
        let held = by_sha.entry(entry.sha.as_str()).or_insert(entry);
        if held.cause.is_none() && entry.cause.is_some() {
            *held = entry;
        }
    }
    by_sha.into_values().collect()
}

fn tally(landings: &[&Judged]) -> BTreeMap<Class, Tally> {
    let mut classes: BTreeMap<Class, Tally> = BTreeMap::new();
    for landing in landings {
        let tally = classes.entry(Class::of_landing(&landing.files)).or_default();
        tally.landings += 1;
        match landing.cause {
            Some(Cause::Reverted) => tally.reverted += 1,
            Some(Cause::FixedForward) => tally.fixed_forward += 1,
            Some(Cause::Culprit) => tally.culprit += 1,
            None => continue,
        }
        if let Some(subject) = &landing.fixed_by {
            tally.fixes.push((landing.at, subject.chars().take(SUBJECT_CHARS).collect()));
        }
    }
    classes
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 { "n/a".into() } else { format!("{:.0}%", part as f64 / whole as f64 * 100.0) }
}

pub fn render(landings: &[&Judged], window: &str) -> String {
    let classes = tally(landings);
    let reworked: usize = classes.values().map(Tally::reworked).sum();
    let mut out = String::new();
    let _ = writeln!(out, "north scoreboard rework: {reworked} of {} landings that reached main {window} were reworked within 48 h ({}), by class of change", landings.len(), percent(reworked, landings.len()));
    let mut ordered: Vec<(Class, Tally)> = classes.into_iter().collect();
    ordered.sort_by_key(|(_, tally)| std::cmp::Reverse(tally.reworked()));
    for (class, mut tally) in ordered {
        let _ = writeln!(
            out,
            "- {}: {} of {} ({}): reverted {}, fixed forward {}, red-main culprit {}",
            class.name(),
            tally.reworked(),
            tally.landings,
            percent(tally.reworked(), tally.landings),
            tally.reverted,
            tally.fixed_forward,
            tally.culprit
        );
        tally.fixes.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        for (_, subject) in tally.fixes.iter().take(EXAMPLES) {
            let _ = writeln!(out, "    fixed by: {subject}");
        }
    }
    out
}

pub fn run(sources: &Sources, weeks: [Window; 2]) -> Result<String, String> {
    let threads = open(&sources.threads)?;
    let rework = Rework::read(&threads, &sources.places.rework).map_err(|why| format!("rework: {why}"))?;
    let held = landings(rework.judged.values().filter(|judged| weeks[0].holds(judged.at)));
    Ok(render(&held, "in the last 7 days"))
}

#[cfg(test)]
#[path = "cause_tests.rs"]
mod tests;
