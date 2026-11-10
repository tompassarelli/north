//! What git history says about the registry's machines: the incident fixes in each machine's code, which the
//! machine-fix detector files as boxes, and a replay of that trigger against calendar and churn sweeps.
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use north_core::error::NorthResult;

use super::{CHARTS, MACHINES, git};

const DAY: u64 = 24 * 3600;
const WEEK: u64 = 7 * DAY;
const MONTH: u64 = 30 * DAY;
const LEADS: [u64; 2] = [3 * DAY, WEEK];

struct Commit<'a> {
    sha: &'a str,
    at: u64,
    files: Vec<&'a str>,
}

/// Whether a commit subject narrates a dated incident, as north's fix commits do: "... on 2026-11-07 ...".
fn narrates(subject: &str) -> bool {
    subject.match_indices(" on ").any(|(at, _)| {
        let date = &subject.as_bytes()[at + 4..];
        date.len() >= 10 && date[..10].iter().enumerate().all(|(index, byte)| if index == 4 || index == 7 { *byte == b'-' } else { byte.is_ascii_digit() })
    })
}

fn since(days: u32) -> String {
    format!("--since={days}.days")
}

fn logged(root: &Path, days: u32) -> NorthResult<String> {
    git(root, &["log", &since(days), "--name-only", "--format=%x01%h %ct %s"])
}

fn incident(text: &str) -> Option<Commit<'_>> {
    let (head, files) = text.split_once('\n')?;
    let (sha, rest) = head.split_once(' ')?;
    let (at, subject) = rest.split_once(' ')?;
    narrates(subject).then_some(Commit { sha, at: at.parse().ok()?, files: files.lines().collect() })
}

/// The commits of `log` that narrate an incident, newest first.
fn incidents(log: &str) -> Vec<Commit<'_>> {
    log.split('\u{1}').filter_map(incident).collect()
}

/// Whether `commit` changed the code of machine `index`. A table file several machines share counts for the first
/// of them.
fn touches(index: usize, commit: &Commit) -> bool {
    let machine = &MACHINES[index];
    let owns = !MACHINES[..index].iter().any(|earlier| earlier.source == machine.source);
    commit.files.iter().any(|file| machine.code.iter().any(|path| file.starts_with(path)) || (owns && *file == machine.source))
}

/// One line per machine whose code a commit of the last `days` days changed while narrating an incident: the
/// machine, whether it is modelled, the week of its newest such commit and the commits, newest first. A modelled
/// machine counts only commits newer than the last change of its chart, which is when its checked model last changed.
pub(super) fn fixes(root: &Path, days: u32) -> NorthResult<Vec<String>> {
    let log = logged(root, days)?;
    let commits = incidents(&log);
    let mut lines = Vec::new();
    for (index, machine) in MACHINES.iter().enumerate() {
        let chart = format!("{CHARTS}/{}.md", machine.name);
        let checked = if machine.explore.is_some() { git(root, &["log", "-1", &since(days), "--format=%ct", "--", &chart])?.trim().parse().unwrap_or(0) } else { 0 };
        let fixed: Vec<&Commit> = commits.iter().filter(|commit| commit.at > checked && touches(index, commit)).collect();
        if let Some(newest) = fixed.first() {
            let shas: Vec<&str> = fixed.iter().map(|commit| commit.sha).collect();
            let model = if machine.explore.is_some() { "modelled" } else { "unmodelled" };
            lines.push(format!("machine-fix: {} {model} week {} fixes {}", machine.name, newest.at / WEEK, shas.join(",")));
        }
    }
    Ok(lines)
}

/// When a sweep placed by churn fires: as many sweeps as `sweeps`, each when an equal share of the period's changed
/// lines has landed.
fn churn(root: &Path, days: u32, sweeps: usize) -> NorthResult<Vec<u64>> {
    let log = git(root, &["log", &since(days), "--reverse", "--shortstat", "--format=%x01%ct"])?;
    let changed: Vec<(u64, u64)> = log
        .split('\u{1}')
        .filter_map(|commit| {
            let (at, stat) = commit.split_once('\n')?;
            let lines = stat.split(',').skip(1).filter_map(|part| part.trim().split(' ').next()?.parse::<u64>().ok()).sum();
            Some((at.trim().parse().ok()?, lines))
        })
        .collect();
    let share = (changed.iter().map(|(_, lines)| lines).sum::<u64>() / u64::try_from(sweeps.max(1)).unwrap_or(1)).max(1);
    let mut held = 0;
    Ok(changed
        .into_iter()
        .filter_map(|(at, lines)| {
            held += lines;
            (held >= share).then(|| {
                held = 0;
                at
            })
        })
        .collect())
}

/// The reviews the detector files: per machine, the first fix of each week.
fn signalled(fixed: &[Vec<(u64, &str)>]) -> Vec<Vec<u64>> {
    let firsts = |times: &Vec<(u64, &str)>| {
        let mut firsts: Vec<u64> = Vec::new();
        for (at, _) in times {
            if firsts.last().is_none_or(|first| first / WEEK != at / WEEK) {
                firsts.push(*at);
            }
        }
        firsts
    };
    fixed.iter().map(firsts).collect()
}

fn preceded<'a>(reviews: &[u64], fixed: &[(u64, &'a str)], lead: u64) -> Vec<&'a str> {
    fixed.iter().filter(|(at, _)| reviews.iter().any(|review| review < at && *at <= review + lead)).map(|(_, sha)| *sha).collect()
}

fn scored(name: &str, reviews: &[Vec<u64>], fixed: &[Vec<(u64, &str)>]) -> String {
    let count: usize = reviews.iter().map(Vec::len).sum();
    let idle: usize = reviews.iter().zip(fixed).map(|(reviews, fixed)| reviews.iter().filter(|review| preceded(&[**review], fixed, WEEK).is_empty()).count()).sum();
    let [soon, late] = LEADS.map(|lead| reviews.iter().zip(fixed).map(|(reviews, fixed)| preceded(reviews, fixed, lead).len()).sum::<usize>());
    format!("replay: {name}: {count} machine reviews, {idle} with no fix of that machine in the week after, {soon} fixes preceded within 3 days and {late} within 7")
}

/// The detector replayed over the last `days` days beside three sweeps that review every machine: `signal` is one
/// review per machine and week at the week's first incident fix, `weekly` and `monthly` sweep on the calendar, and
/// `churn` sweeps as often as `weekly`, each time an equal share of the period's changed lines has landed. A fix is
/// preceded when a review of its machine fired in the days before it. Then one line per machine the signal
/// reviewed, naming the fixes that landed within a week of its reviews.
pub(super) fn replay(root: &Path, days: u32) -> NorthResult<Vec<String>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let start = now.saturating_sub(u64::from(days) * DAY);
    let log = logged(root, days)?;
    let commits = incidents(&log);
    let fixed: Vec<Vec<(u64, &str)>> = (0..MACHINES.len()).map(|index| commits.iter().rev().filter(|commit| touches(index, commit)).map(|commit| (commit.at, commit.sha)).collect()).collect();
    let calendar = |step: u64| (start..now).step_by(usize::try_from(step).unwrap_or(usize::MAX)).collect::<Vec<u64>>();
    let every = |sweeps: Vec<u64>| vec![sweeps; MACHINES.len()];
    let weekly = calendar(WEEK);
    let churned = churn(root, days, weekly.len())?;
    let signal = signalled(&fixed);
    let total: usize = fixed.iter().map(Vec::len).sum();
    let mut lines = vec![format!("replay: {days} days, {} commits narrate an incident, {total} fixes in the code of {} of {} machines", commits.len(), fixed.iter().filter(|fixes| !fixes.is_empty()).count(), MACHINES.len())];
    lines.push(scored("signal", &signal, &fixed));
    lines.extend([("weekly", weekly), ("monthly", calendar(MONTH)), ("churn", churned)].map(|(name, sweeps)| scored(name, &every(sweeps), &fixed)));
    for ((machine, reviews), fixes) in MACHINES.iter().zip(&signal).zip(&fixed).filter(|((_, reviews), _)| !reviews.is_empty()) {
        lines.push(format!("replay: {}: {} fixes, {} reviews, preceded {}", machine.name, fixes.len(), reviews.len(), preceded(reviews, fixes, WEEK).join(",")));
    }
    Ok(lines)
}
