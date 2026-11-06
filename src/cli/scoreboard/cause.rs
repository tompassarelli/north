//! `north scoreboard --rework [--since 7d]`: the reworked landings of a window by the judge's rule and by class of
//! change, each with the main commit that triggered the rework, so the fixes can target the rule or class that
//! reworks most.
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{Sources, Window, open};
use crate::foundation::clock::Timestamp;
use crate::threads::calibration::rework::{Cause, Judged, Rework, Rule};

const SUBJECT_CHARS: usize = 90;
const SHA_CHARS: usize = 9;

const DEFAULT_SINCE: &str = "7d";

/// A `--rework [--since 7d]` request: the window of landings to break down.
pub struct Request {
    since_s: i64,
    label: String,
}

impl Request {
    /// `None` when the arguments are not a rework request.
    pub fn parse(arguments: &[String]) -> Option<Self> {
        let (rework, rest) = arguments.split_first()?;
        let since = match rest {
            [] => DEFAULT_SINCE,
            [flag, value] if flag == "--since" => value,
            _ => return None,
        };
        (rework == "--rework").then_some(())?;
        Some(Self { since_s: span(since)?, label: since.to_owned() })
    }

    pub fn window(&self, now: i64) -> Window {
        Window { start: now - self.since_s, end: now }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

fn span(text: &str) -> Option<i64> {
    let (count, unit) = text.char_indices().next_back().map(|(at, _)| text.split_at(at))?;
    let count: i64 = count.parse().ok().filter(|count| *count > 0)?;
    Some(count * match unit {
        "d" => 86_400,
        "h" => 3_600,
        _ => return None,
    })
}

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
        tally.fixes.push((landing.at, trigger_line(landing)));
    }
    classes
}

fn short(sha: &str) -> String {
    sha.chars().take(SHA_CHARS).collect()
}

fn trigger_line(landing: &Judged) -> String {
    let rule = landing.rule.map_or("rule not recorded", Rule::label);
    match &landing.trigger {
        Some(trigger) => {
            let subject: String = trigger.subject.chars().take(SUBJECT_CHARS).collect();
            format!("{} reworked by {} {subject} [{rule}]", short(&landing.sha), short(&trigger.sha))
        }
        None => format!("{} named by a red-main issue [{rule}]", short(&landing.sha)),
    }
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 { "n/a".into() } else { format!("{:.0}%", part as f64 / whole as f64 * 100.0) }
}

fn render_repo(repo: &str, landings: &[&Judged]) -> String {
    let classes = tally(landings);
    let reworked: usize = classes.values().map(Tally::reworked).sum();
    let mut out = String::new();
    let _ = writeln!(out, "{repo}: {reworked} of {} judged landings reworked within 48 h ({})", landings.len(), percent(reworked, landings.len()));
    let by_rule: Vec<String> = Rule::ALL.iter().map(|rule| format!("{} {}", rule.label(), landings.iter().filter(|landing| landing.rule == Some(*rule)).count())).collect();
    let _ = writeln!(out, "  by rule: {}", by_rule.join(", "));
    let mut ordered: Vec<(Class, Tally)> = classes.into_iter().collect();
    ordered.sort_by_key(|(_, tally)| std::cmp::Reverse(tally.reworked()));
    let _ = writeln!(out, "  by class of change:");
    for (class, mut tally) in ordered {
        let _ = writeln!(
            out,
            "  - {}: {} of {} ({}): reverted {}, fixed forward {}, red-main culprit {}",
            class.name(),
            tally.reworked(),
            tally.landings,
            percent(tally.reworked(), tally.landings),
            tally.reverted,
            tally.fixed_forward,
            tally.culprit
        );
        tally.fixes.sort_by_key(|(at, _)| *at);
        for (_, line) in &tally.fixes {
            let _ = writeln!(out, "      {line}");
        }
    }
    out
}

pub fn render(landings: &[&Judged], window: &str, now: i64) -> String {
    let mut repos: BTreeMap<&str, Vec<&Judged>> = BTreeMap::new();
    for landing in landings {
        repos.entry(landing.repo.as_str()).or_default().push(landing);
    }
    let mut out = format!("north scoreboard --rework: worker landings that reached main in the last {window}, ending {} today\n", Timestamp::from_unix_seconds(now).local_twelve_hour());
    for (repo, held) in repos {
        out.push_str(&render_repo(repo, &held));
    }
    out
}

pub fn run(sources: &Sources, window: Window, label: &str, now: i64) -> Result<String, String> {
    let threads = open(&sources.threads)?;
    let rework = Rework::read(&threads, &sources.places.rework).map_err(|why| format!("rework: {why}"))?;
    let held = landings(rework.judged.values().filter(|judged| window.holds(judged.at)));
    Ok(render(&held, label, now))
}

#[cfg(test)]
#[path = "cause_tests.rs"]
mod tests;
