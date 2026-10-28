//! Whether issues are closing as fast as the goal needs, and how long landed work waits to close.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::host::{median, seconds};

const WINDOW: f64 = 3.0 * 3600.0;
const UNOWNED_SECONDS: f64 = 900.0;
const DAY: f64 = 86_400.0;

static AUTO_CLOSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Closed by auto-close after ([0-9a-f]{7,64}) landed").unwrap());
static LAND_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:fix(?:e[sd])?|close[sd]?|refs?)\b:?\s+((?:[\w.-]+/)?[\w.-]+)?#(\d+)").unwrap());

pub(crate) struct Closed {
    pub(crate) number: u64,
    pub(crate) closed_at: f64,
    pub(crate) comments: Vec<String>,
}

pub(crate) struct Commit {
    pub(crate) sha: String,
    pub(crate) at: f64,
    pub(crate) message: String,
}

/// A failed lane: when it failed and the issues it names.
pub(crate) struct DeadLane {
    pub(crate) failed_at: f64,
    pub(crate) issues: BTreeSet<u64>,
}

pub(crate) struct Inputs<'a> {
    pub(crate) repo: &'a str,
    pub(crate) now: f64,
    pub(crate) open_count: Option<u64>,
    pub(crate) closed: &'a [Closed],
    pub(crate) main_log: &'a [Commit],
    pub(crate) landings: &'a [f64],
    pub(crate) pushes: &'a [f64],
    pub(crate) target: Option<f64>,
    pub(crate) dead_lanes: &'a [DeadLane],
    pub(crate) claims: &'a BTreeMap<u64, f64>,
    pub(crate) pending_since: &'a [f64],
}

#[derive(Debug, Serialize)]
pub(crate) struct Throughput {
    pub(crate) target: Option<f64>,
    pub(crate) closed_per_hour: Option<f64>,
    pub(crate) needed_per_hour: Option<f64>,
    pub(crate) land_to_close_minutes: Option<f64>,
    pub(crate) land_ratio_3h: Option<f64>,
    pub(crate) failed_unowned: usize,
    pub(crate) oldest_lane_wait_minutes: Option<f64>,
}

/// The issues a commit message closes or refers to in `repo`; a reference qualified with another
/// repository does not count.
pub(crate) fn issue_refs(text: &str, repo: &str) -> BTreeSet<u64> {
    LAND_REF
        .captures_iter(text)
        .filter(|found| found.get(1).is_none_or(|qualifier| qualifier.as_str().rsplit('/').next() == Some(repo)))
        .filter_map(|found| found[2].parse().ok())
        .collect()
}

pub(crate) fn branch_numbers(name: &str) -> BTreeSet<u64> {
    name.split(['-', '_', '/', '#']).filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())).filter_map(|part| part.parse().ok()).collect()
}

fn landed_at(inputs: &Inputs, item: &Closed, referenced: &BTreeMap<u64, Vec<f64>>) -> Option<f64> {
    let named = item.comments.iter().filter_map(|body| AUTO_CLOSE.captures(body)).map(|found| found[1].to_owned()).next_back();
    let by_comment = named.and_then(|prefix| inputs.main_log.iter().find(|commit| commit.sha.starts_with(&prefix)).map(|commit| commit.at));
    by_comment.or_else(|| referenced.get(&item.number)?.iter().copied().filter(|at| *at <= item.closed_at).max_by(f64::total_cmp))
}

pub(crate) fn throughput(inputs: &Inputs) -> Throughput {
    let now = inputs.now;
    let recent: Vec<&Closed> = inputs.closed.iter().filter(|item| (now - DAY..=now).contains(&item.closed_at)).collect();
    let closed_per_hour = inputs.open_count.map(|_| recent.iter().filter(|item| item.closed_at >= now - WINDOW).count() as f64 / 3.0);
    let hours = inputs.target.map_or(0.0, |target| (target - now) / 3600.0);
    let needed_per_hour = inputs.open_count.filter(|_| hours > 0.0).map(|open| open as f64 / hours);
    let mut referenced: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
    for commit in inputs.main_log {
        for number in issue_refs(&commit.message, inputs.repo) {
            referenced.entry(number).or_default().push(commit.at);
        }
    }
    let samples: Vec<f64> = recent.iter().filter_map(|item| Some((item.closed_at - landed_at(inputs, item, &referenced)?).max(0.0) / 60.0)).collect();
    let within = |times: &[f64]| times.iter().filter(|at| (now - WINDOW..=now).contains(*at)).count();
    let pushed = within(inputs.pushes);
    let unowned = inputs
        .dead_lanes
        .iter()
        .filter(|lane| now - lane.failed_at > UNOWNED_SECONDS && !lane.issues.iter().any(|number| inputs.claims.get(number).is_some_and(|claimed| *claimed >= lane.failed_at)))
        .count();
    Throughput {
        target: inputs.target,
        closed_per_hour,
        needed_per_hour,
        land_to_close_minutes: median(&samples),
        land_ratio_3h: (pushed > 0).then(|| within(inputs.landings) as f64 / pushed as f64),
        failed_unowned: unowned,
        oldest_lane_wait_minutes: inputs.pending_since.iter().copied().min_by(f64::total_cmp).map(|since| (now - since).max(0.0) / 60.0),
    }
}

pub(crate) fn closed_from(number: u64, closed_at: &str, comments: Vec<String>) -> Option<Closed> {
    Some(Closed { number, closed_at: seconds(closed_at)?, comments })
}
