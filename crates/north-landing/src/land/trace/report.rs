use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

use super::profile::{Profile, Split, Trace, percentile};
use super::{Category, Span, Spans};
use crate::foundation::clock::Timestamp;
use crate::land::lander::state_home;
use crate::land::telemetry::epoch;

const USAGE: &str = "usage: north land profile [--since DURATION] [--spans FILE]... [--landing BRANCH] [--repo NAME] [--last N] [--metric landing-p50|landing-p90|landing-projected|verification-p50|verification-p90]";
const TOP: usize = 10;

pub fn duration_seconds(text: &str) -> Option<f64> {
    let split = text.find(|character: char| !character.is_ascii_digit() && character != '.')?;
    let (value, unit) = text.split_at(split);
    let scale = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        _ => return None,
    };
    Some(value.parse::<f64>().ok()? * scale)
}

pub fn every_queue() -> Vec<PathBuf> {
    let root = state_home().join("north/land-queue");
    let mut files: Vec<PathBuf> = fs::read_dir(root).into_iter().flatten().filter_map(Result::ok).map(|entry| Spans::beside(&entry.path().join("logs")).path().to_owned()).filter(|path| path.is_file()).collect();
    files.sort();
    files
}

/// The tickets waiting in the land queue whose spans file is `spans`.
fn queued_behind(spans: &std::path::Path) -> Option<usize> {
    let queue = spans.parent()?.parent()?;
    Some(crate::land::queue::pending(queue).map_or(0, |tickets| tickets.len()))
}

/// The repository a land queue serves, from the origin its tickets name: a done ticket's worktree is usually gone, so the checkout mapping cannot say.
pub fn queue_repo(spans: &std::path::Path) -> Option<String> {
    let queue = spans.parent()?.parent()?;
    let tickets = [queue.to_path_buf(), queue.join("done")];
    let ticket = tickets.iter().filter_map(|dir| fs::read_dir(dir).ok()).flatten().filter_map(Result::ok).map(|entry| entry.path()).find(|path| path.extension().is_some_and(|extension| extension == "json"))?;
    let origin = serde_json::from_str::<serde_json::Value>(&fs::read_to_string(ticket).ok()?).ok()?["origin"].as_str()?.to_owned();
    Some(origin.trim_end_matches('/').trim_end_matches(".git").rsplit('/').next()?.to_owned())
}

fn serves(spans: &std::path::Path, repo: &str) -> bool {
    queue_repo(spans).is_some_and(|name| name == repo)
}

fn seconds(value: f64) -> String {
    format!("{value:.0} s")
}

fn categories(split: &Split, total: f64) -> String {
    Category::ORDER
        .iter()
        .filter(|category| split.get(**category) >= 0.5)
        .map(|category| {
            let value = split.get(*category);
            let crates = if *category == Category::Test && !split.crates.is_empty() {
                let mut crates: Vec<_> = split.crates.iter().filter(|(_, value)| **value >= 0.5).collect();
                crates.sort_by(|left, right| right.1.total_cmp(left.1));
                format!(" ({})", crates.iter().map(|(name, value)| format!("{name} {value:.0}")).collect::<Vec<_>>().join(", "))
            } else {
                String::new()
            };
            let share = if total > 0.0 { value / total * 100.0 } else { 0.0 };
            format!("{} {value:.0} s {share:.0}%{crates}", category.label())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

pub fn render(profiles: &[Profile<'_>], since: f64) -> String {
    let mut out = String::new();
    let walls: Vec<f64> = profiles.iter().map(Profile::wall).collect();
    let _ = writeln!(out, "north land profile: {} landing{} since {}", profiles.len(), if profiles.len() == 1 { "" } else { "s" }, Timestamp::from_unix_seconds_f64(since).local_twelve_hour());
    if profiles.is_empty() {
        return out;
    }
    let _ = writeln!(out, "queued_at to push: p50 {}, p90 {}, sum {}", seconds(percentile(&walls, 0.5)), seconds(percentile(&walls, 0.9)), seconds(walls.iter().sum()));
    let verified: Vec<f64> = profiles.iter().map(|profile| profile.after_wall).filter(|wall| *wall > 0.0).collect();
    if !verified.is_empty() {
        let _ = writeln!(out, "verification after the push: p50 {}, p90 {} over {} landing{}", seconds(percentile(&verified, 0.5)), seconds(percentile(&verified, 0.9)), verified.len(), if verified.len() == 1 { "" } else { "s" });
    }
    let counted = |verdicts: usize| profiles.iter().filter(|profile| profile.verdicts.len() == verdicts).count();
    let many = profiles.len() - counted(0) - counted(1);
    let _ = writeln!(out, "verification verdicts: {} landing{} with one, {} with none yet, {many} with more than one", counted(1), if counted(1) == 1 { "" } else { "s" }, counted(0));
    let _ = writeln!(out, "\neach landing, queued_at to push:");
    for profile in profiles {
        let landing = profile.landing;
        let landed = landing.attribute("landed").unwrap_or_default();
        let wall = profile.wall();
        let total = profile.before.total();
        let _ = writeln!(out, "- {} landed {} at {}: wall {}, categories sum {} ({:+.1}%)", landing.name, short(landed), Timestamp::from_unix_seconds_f64(landing.end).local_twelve_hour(), seconds(wall), seconds(total), if wall > 0.0 { (total - wall) / wall * 100.0 } else { 0.0 });
        let _ = writeln!(out, "  {}", categories(&profile.before, total));
        if profile.after_wall > 0.0 {
            let _ = writeln!(out, "  after push, {} of verification: {}", seconds(profile.after_wall), categories(&profile.after, profile.after.total()));
        }
        let verdict = if profile.verdicts.is_empty() { "none yet".to_owned() } else { profile.verdicts.join(", ") };
        let _ = writeln!(out, "  verdict: {verdict}");
        let cached = |state: &str| profile.spans.iter().filter(|span| span.attribute("cache") == Some(state)).count();
        if cached("hit") > 0 {
            let _ = writeln!(out, "  verdict cache: {} check(s) reused a pass, {} ran", cached("hit"), cached("miss"));
        }
    }
    let mut summed = Split::default();
    let mut after = Split::default();
    for profile in profiles {
        for (category, value) in &profile.before.categories {
            *summed.categories.entry(*category).or_default() += value;
        }
        for (name, value) in &profile.before.crates {
            *summed.crates.entry(name.clone()).or_default() += value;
        }
        for (category, value) in &profile.after.categories {
            *after.categories.entry(*category).or_default() += value;
        }
        for (name, value) in &profile.after.crates {
            *after.crates.entry(name.clone()).or_default() += value;
        }
    }
    let total = summed.total();
    let _ = writeln!(out, "\nacross landings, queued_at to push (sum {}, wall sum {}):", seconds(total), seconds(walls.iter().sum()));
    for category in Category::ORDER.iter().filter(|category| summed.get(**category) >= 0.5) {
        let values: Vec<f64> = profiles.iter().map(|profile| profile.before.get(*category)).collect();
        let kind = if category.waiting() { "waiting" } else { "running" };
        let _ = writeln!(out, "- {:<22} {:>8} {:>4.0}%  p50 {:>7}  p90 {:>7}  {kind}", category.label(), seconds(summed.get(*category)), summed.get(*category) / total * 100.0, seconds(percentile(&values, 0.5)), seconds(percentile(&values, 0.9)));
    }
    if !summed.crates.is_empty() {
        let mut crates: Vec<_> = summed.crates.iter().collect();
        crates.sort_by(|left, right| right.1.total_cmp(left.1));
        let _ = writeln!(out, "  test run by crate: {}", crates.iter().map(|(name, value)| format!("{name} {value:.0} s")).collect::<Vec<_>>().join(", "));
    }
    if after.total() > 0.0 {
        let _ = writeln!(out, "\nafter push, verification across landings (sum {}):", seconds(after.total()));
        let _ = writeln!(out, "  {}", categories(&after, after.total()));
    }
    let parents: std::collections::HashSet<&str> = profiles.iter().flat_map(|profile| profile.spans.iter().filter_map(|span| span.parent.as_deref())).collect();
    let mut spans: Vec<(&Span, &str)> = profiles.iter().flat_map(|profile| profile.spans.iter().filter(|span| span.category != Category::Landing && !parents.contains(span.id.as_str())).map(|span| (*span, profile.landing.name.as_str()))).collect();
    spans.sort_by(|left, right| right.0.seconds().total_cmp(&left.0.seconds()));
    spans.dedup_by(|left, right| left.0.id == right.0.id);
    let _ = writeln!(out, "\ntop {TOP} spans without child spans:");
    for (rank, (span, landing)) in spans.iter().take(TOP).enumerate() {
        let host = if span.host.is_empty() { String::new() } else { format!(" on {}", span.host) };
        let cache = span.attribute("cache").map(|cache| format!(", cache {cache}")).unwrap_or_default();
        let cpu = span.cpu.map(|cpu| format!(", cpu {cpu:.0} s")).unwrap_or_default();
        let _ = writeln!(out, "{}. {} {}: {}{host}{cache}{cpu} ({landing})", rank + 1, seconds(span.seconds()), span.category.label(), span.name);
    }
    out
}

pub fn run(arguments: &[String]) -> u8 {
    let mut since = 3600.0;
    let mut files = Vec::new();
    let mut only = None;
    let mut metric = None;
    let mut repo = None;
    let mut last = None;
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--since", Some(value)) if let Some(parsed) = duration_seconds(value) => since = parsed,
            ("--spans", Some(value)) => files.push(PathBuf::from(value)),
            ("--landing", Some(value)) => only = Some(value.clone()),
            ("--repo", Some(value)) => repo = Some(value.clone()),
            ("--last", Some(value)) if value.parse::<usize>().is_ok_and(|count| count > 0) => last = value.parse().ok(),
            ("--metric", Some(value)) if Metric::parse(value).is_some() => metric = Metric::parse(value),
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    let mut pending = 0;
    if files.is_empty() {
        files = every_queue();
        if let Some(repo) = &repo {
            files.retain(|file| serves(file, repo));
        }
        pending = files.iter().filter_map(|file| queued_behind(file)).sum();
    }
    let spans: Vec<Span> = files.iter().flat_map(|file| Spans::at(file).read()).collect();
    let trace = Trace::of(&spans);
    let from = epoch() - since;
    let profiles: Vec<Profile<'_>> = trace.latest(from, last).into_iter().filter(|landing| only.as_deref().is_none_or(|only| landing.name == only)).map(|landing| trace.profile(landing)).collect();
    if let Some(metric) = metric {
        metric.print(&profiles, pending);
        return 0;
    }
    print!("{}", render(&profiles, from));
    0
}

#[derive(Clone, Copy)]
pub enum Metric {
    Landing(f64),
    Verification(f64),
    Projected,
}

impl Metric {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "landing-p50" => Some(Self::Landing(0.5)),
            "landing-p90" => Some(Self::Landing(0.9)),
            "landing-projected" => Some(Self::Projected),
            "verification-p50" => Some(Self::Verification(0.5)),
            "verification-p90" => Some(Self::Verification(0.9)),
            _ => None,
        }
    }

    fn print(self, profiles: &[Profile<'_>], pending: usize) {
        println!("{:.0}", self.value_behind(profiles, pending));
    }

    pub fn value(self, profiles: &[Profile<'_>]) -> f64 {
        self.value_behind(profiles, 0)
    }

    /// The metric over `profiles`, a lane arriving behind `pending` queued tickets; only the projected wait reads the queue.
    pub fn value_behind(self, profiles: &[Profile<'_>], pending: usize) -> f64 {
        let values: Vec<f64> = match self {
            Self::Landing(_) => profiles.iter().map(Profile::wall).collect(),
            Self::Verification(_) => profiles.iter().map(|profile| profile.after_wall).filter(|wall| *wall > 0.0).collect(),
            Self::Projected => profiles.iter().map(|profile| (profile.wall() - profile.before.get(Category::Queue)).max(0.0)).collect(),
        };
        match self {
            Self::Landing(fraction) | Self::Verification(fraction) => percentile(&values, fraction),
            #[allow(clippy::cast_precision_loss)]
            Self::Projected => percentile(&values, 0.5) * (pending + 1) as f64,
        }
    }
}
