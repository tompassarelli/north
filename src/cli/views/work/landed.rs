use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item};

use crate::foundation::clock::{Timestamp, local_today};
use crate::platform::{git, orchestration};
use crate::threads::catalog::checkout;

pub const FLOOR: u64 = 2_000;
pub const GOAL: u64 = 5_000;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;
const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';

pub struct Commit {
    pub repo: String,
    pub sha: String,
    pub committed: i64,
    pub message: String,
}

#[derive(Default)]
pub struct Tally {
    pub today: u64,
    pub by_repo: BTreeMap<String, u64>,
    pub last_hour: u64,
    pub projection: u64,
}

pub struct Landed {
    pub day: String,
    pub tally: Tally,
    pub unread: Vec<String>,
}

pub fn read(at: Timestamp) -> Result<Landed, String> {
    let policy = orchestration::read()?;
    let now = at.unix_seconds();
    let day_start = i64::try_from(at.local_day_start_ms() / 1000).unwrap_or_default();
    let since = day_start.min(now - HOUR);
    let mut commits = Vec::new();
    let mut unread = Vec::new();
    for repo in landing_repos(&policy) {
        let dir = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code").join(checkout(&repo)).join("main");
        if !dir.is_dir() {
            continue;
        }
        match commits_of(&repo, &dir, since) {
            Ok(found) => commits.extend(found),
            Err(why) => unread.push(format!("{repo}: {why}")),
        }
    }
    Ok(Landed { day: local_today().to_string(), tally: tally(&commits, day_start, now), unread })
}

fn landing_repos(policy: &DocumentMut) -> Vec<String> {
    let mut repos: Vec<String> = policy
        .get("projects")
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|projects| projects.iter())
        .flat_map(|(_, project)| project.get("repos").and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect::<Vec<_>>())
        .collect();
    repos.sort();
    repos.dedup();
    repos
}

fn commits_of(repo: &str, dir: &Path, since: i64) -> Result<Vec<Commit>, String> {
    let since = format!("--since=@{since}");
    let output = git::run(dir, &["log", "origin/main", since.as_str(), "--format=%H%x1f%ct%x1f%B%x1e"]).map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).split(RECORD).filter_map(|record| parse(repo, record)).collect())
}

fn parse(repo: &str, record: &str) -> Option<Commit> {
    let mut fields = record.splitn(3, FIELD);
    Some(Commit { repo: repo.to_owned(), sha: fields.next()?.trim().to_owned(), committed: fields.next()?.parse().ok()?, message: fields.next()?.to_owned() })
}

fn reverted(message: &str) -> impl Iterator<Item = &str> {
    message.lines().filter_map(|line| {
        let named = line.strip_prefix("Reverts:").or_else(|| line.split_once("This reverts commit ").map(|(_, sha)| sha))?;
        named.split_whitespace().next().map(|sha| sha.trim_end_matches('.'))
    })
}

pub(super) fn tally(commits: &[Commit], day_start: i64, now: i64) -> Tally {
    let mut excluded: BTreeSet<&str> = BTreeSet::new();
    for commit in commits {
        let named: Vec<&str> = reverted(&commit.message).collect();
        if !named.is_empty() {
            excluded.insert(commit.sha.as_str());
            excluded.extend(named);
        }
    }
    let mut tally = Tally::default();
    for commit in commits.iter().filter(|commit| !excluded.contains(commit.sha.as_str())) {
        if commit.committed >= day_start {
            tally.today += 1;
            *tally.by_repo.entry(commit.repo.clone()).or_default() += 1;
        }
        if commit.committed > now - HOUR {
            tally.last_hour += 1;
        }
    }
    let rest_of_day_hours = (day_start + DAY - now).max(0) as f64 / HOUR as f64;
    tally.projection = tally.today + (tally.last_hour as f64 * rest_of_day_hours).round() as u64;
    tally
}

impl Landed {
    pub fn lines(&self) -> String {
        let tally = &self.tally;
        let repos: Vec<String> = tally.by_repo.iter().map(|(repo, count)| format!("{repo} {count}")).collect();
        let by_repo = if repos.is_empty() { String::new() } else { format!(" ({})", repos.join(", ")) };
        let mut text = format!(
            "Landed today ({}): {} commits{by_repo}\nLast hour: {} landed, {} per hour\nDay projection: {} at this rate; floor {FLOOR} {}, goal {GOAL} {}\n",
            self.day,
            tally.today,
            tally.last_hour,
            tally.last_hour,
            tally.projection,
            against(tally.projection, FLOOR),
            against(tally.projection, GOAL),
        );
        if !self.unread.is_empty() {
            text.push_str(&format!("Unread repos: {}\n", self.unread.join("; ")));
        }
        text
    }

    pub fn json(&self) -> Value {
        let tally = &self.tally;
        json!({
            "day": self.day,
            "commits": tally.today,
            "by_repo": tally.by_repo,
            "last_hour": tally.last_hour,
            "projection": tally.projection,
            "floor": FLOOR,
            "goal": GOAL,
            "unread": self.unread,
        })
    }
}

fn against(projection: u64, target: u64) -> String {
    if projection >= target { format!("met by {}", projection - target) } else { format!("short by {}", target - projection) }
}
