use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use super::{BetIssue, Build, CapacityRun, CloudSession, IssueKind, LimitStop, Load, Place, ReleaseIssue, Size};
use crate::foundation::clock::Timestamp;

const EPICS: [u32; 7] = [1, 7, 8, 9, 10, 11, 12];
const TOM_ONLY: [u32; 3] = [2, 58, 59];

pub(crate) fn seconds(text: &str) -> Option<i64> {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds)
}

pub(crate) fn capacity_runs(log: &str, place: Place, since: i64) -> Vec<CapacityRun> {
    log.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|row| {
            let at = seconds(row["at"].as_str()?)?;
            (at >= since).then(|| CapacityRun {
                place,
                at,
                owner: row["owner"].as_str().unwrap_or_default().to_owned(),
                cpu_seconds: row["cpuSeconds"].as_f64().unwrap_or(0.0),
            })
        })
        .collect()
}

pub(crate) fn cloud_sessions(listing: &str) -> Vec<CloudSession> {
    listing
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 5 || fields[2] != "anthropic_cloud" {
                return None;
            }
            let created = (fields.len() >= 6).then(|| seconds(fields[4])).flatten();
            let title = if created.is_some() { fields[5..].join("\t") } else { fields[4..].join("\t") };
            Some(CloudSession {
                id: fields[0].to_owned(),
                status: fields[1].to_owned(),
                placed_at: created.or_else(|| seconds(fields[3]))?,
                title,
            })
        })
        .collect()
}

pub(crate) struct Push {
    pub at: i64,
    pub repo: String,
    pub sha: String,
}

pub(crate) fn pushes(table: &str, since: i64) -> Vec<Push> {
    let mut seen = std::collections::BTreeSet::new();
    table
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let at = fields.first()?.parse::<i64>().ok()?;
            let sha = fields.get(3).filter(|sha| sha.len() >= 7)?;
            let repo = fields.get(1)?.to_string();
            (at >= since && seen.insert(sha.to_string())).then(|| Push { at, repo, sha: sha.to_string() })
        })
        .collect()
}

pub(crate) fn issue_refs(message: &str) -> Vec<u32> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"(?:^|[^\w/])(?:[\w.-]+)?#(\d{1,6})\b").expect("issue reference pattern"));
    pattern.captures_iter(message).filter_map(|c| c[1].parse().ok()).collect()
}

pub(crate) fn release_issues(listing: &Value) -> Vec<ReleaseIssue> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| {
            let number = u32::try_from(issue["number"].as_u64()?).ok()?;
            let kind = if EPICS.contains(&number) {
                IssueKind::Epic
            } else if TOM_ONLY.contains(&number) {
                IssueKind::TomOnly
            } else {
                IssueKind::Work
            };
            Some(ReleaseIssue {
                number,
                open: issue["state"].as_str().is_some_and(|state| state.eq_ignore_ascii_case("open")),
                closed_at: issue["closedAt"].as_str().and_then(seconds),
                kind,
                size: size(issue),
            })
        })
        .collect()
}

pub(crate) fn dividend_ids(body: &str) -> Vec<String> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"(?m)^\s*\**Dividend:\**.*?north ops dividend ([a-z0-9][a-z0-9-]*)").expect("dividend line pattern"));
    pattern.captures_iter(body).map(|c| c[1].to_owned()).collect()
}

pub(crate) fn bet_issues(repo: &str, listing: &Value) -> Vec<BetIssue> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| {
            Some(BetIssue {
                reference: format!("{repo}#{}", issue["number"].as_u64()?),
                title: issue["title"].as_str().unwrap_or_default().to_owned(),
                open: issue["state"].as_str().is_some_and(|state| state.eq_ignore_ascii_case("open")),
                closed_at: issue["closedAt"].as_str().and_then(seconds),
                size: size(issue),
                dividends: dividend_ids(issue["body"].as_str().unwrap_or_default()),
            })
        })
        .collect()
}

pub(crate) fn builds(log: &str, since: i64) -> Vec<Build> {
    log.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            Some(Build { at: seconds(fields.first()?)?, land_to_playable_seconds: fields.get(4)?.parse().ok()? })
        })
        .filter(|build| build.at >= since)
        .collect()
}

pub(crate) fn load(loadavg: &str, cores: u32) -> Option<Load> {
    let mut fields = loadavg.split_whitespace().map(str::parse::<f64>);
    Some(Load { one: fields.next()?.ok()?, five: fields.next()?.ok()?, fifteen: fields.next()?.ok()?, cores })
}

pub(crate) fn lock_holder(text: &str) -> Option<(u32, String)> {
    let rest = text.trim().strip_prefix("pid ")?;
    let (pid, dir) = rest.split_once(" in ")?;
    Some((pid.parse().ok()?, dir.to_owned()))
}

pub(crate) struct Ticket {
    pub since: i64,
    pub pid: u32,
    pub dir: String,
    pub branch: String,
    pub session: Option<String>,
}

pub(crate) fn ticket(name: &str, content: &str) -> Option<Ticket> {
    let (micros, pid) = name.trim_start_matches('0').split_once('-')?;
    let fields: Vec<&str> = content.split('\t').map(str::trim).collect();
    Some(Ticket {
        since: micros.parse::<i64>().ok()? / 1_000_000,
        pid: pid.parse().ok()?,
        dir: fields.first().copied().unwrap_or_default().to_owned(),
        branch: fields.get(1).copied().unwrap_or_default().to_owned(),
        session: fields.get(3).filter(|session| !session.is_empty()).map(|session| session.to_string()),
    })
}

pub(crate) fn repo_of(dir: &str) -> Option<String> {
    let mut parts = dir.split('/').skip_while(|part| *part != "code");
    parts.next()?;
    parts.next().filter(|repo| !repo.is_empty()).map(str::to_owned)
}

pub(crate) fn commit_batch(batch: &[u8]) -> std::collections::BTreeMap<String, (i64, String)> {
    let mut commits = std::collections::BTreeMap::new();
    let mut rest = batch;
    while let Some(end) = rest.iter().position(|byte| *byte == b'\n') {
        let header = String::from_utf8_lossy(&rest[..end]).into_owned();
        rest = &rest[end + 1..];
        let fields: Vec<&str> = header.split(' ').collect();
        let Some(size) = fields.get(2).and_then(|size| size.parse::<usize>().ok()).filter(|_| fields[1] == "commit") else { continue };
        let Some(body) = rest.get(..size) else { break };
        if let Some(commit) = commit_fields(&String::from_utf8_lossy(body)) {
            commits.insert(fields[0].to_owned(), commit);
        }
        rest = rest.get(size + 1..).unwrap_or_default();
    }
    commits
}

fn commit_fields(body: &str) -> Option<(i64, String)> {
    let (headers, message) = body.split_once("\n\n").unwrap_or((body, ""));
    let author = headers.lines().find_map(|line| line.strip_prefix("author "))?;
    let at = author.rsplitn(3, ' ').nth(1)?.parse().ok()?;
    Some((at, message.to_owned()))
}

fn size(issue: &Value) -> Option<Size> {
    Size::from_labels(issue["labels"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()))
}

pub(crate) fn limit_stop(stream_tail: &str, session: &str, account: &str) -> Option<LimitStop> {
    let frames: Vec<Value> = stream_tail.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let last_limit = frames.iter().rposition(|frame| frame["type"] == "rate_limit_event")?;
    let info = &frames[last_limit]["rate_limit_info"];
    if info["status"] != "rejected" {
        return None;
    }
    let resumed = frames[last_limit..].iter().any(|frame| frame["type"] == "system" && frame["subtype"] == "init");
    (!resumed).then(|| LimitStop { session: session.to_owned(), account: account.to_owned(), resets_at: info["resetsAt"].as_i64().unwrap_or_default() })
}

pub(crate) fn check_runs(log: &str, since: i64) -> Vec<i64> {
    log.lines().filter_map(|line| line.split('\t').next()?.parse::<i64>().ok()).filter(|at| *at >= since).collect()
}

pub(crate) fn brief_size(brief: &str) -> Option<(String, Size)> {
    static ITEM: OnceLock<Regex> = OnceLock::new();
    static ETA: OnceLock<Regex> = OnceLock::new();
    let item = ITEM.get_or_init(|| Regex::new(r"(?m)^Item:\s*([a-z0-9_-]+#\d+)").expect("item pattern"));
    let eta = ETA.get_or_init(|| Regex::new(r"(?m)^ETA:\s*(\d+)\s*min").expect("eta pattern"));
    let reference = item.captures(brief)?[1].to_owned();
    let minutes = eta.captures(brief)?[1].parse().ok()?;
    Some((reference, Size::for_eta_minutes(minutes)))
}
