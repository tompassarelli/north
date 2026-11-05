//! Rework: a landed worker run whose main commit is reverted, fixed forward or named a red-main culprit within
//! [`REWORK_WINDOW_S`] of reaching main. A reworked run is not kept.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;
use crate::platform::run_roles::worker_where;
use crate::threads::catalog::CATALOG;

pub const REWORK_WINDOW_S: i64 = 48 * 3600;

static SHA_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b").expect("sha pattern"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Reverted,
    FixedForward,
    Culprit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub sha: String,
    pub at: i64,
    pub subject: String,
    pub body: String,
    pub files: Vec<String>,
}

/// A worker run's landing as the ledger records it.
#[derive(Clone, Debug)]
pub struct Landing {
    pub agent: String,
    pub item: Option<String>,
    pub sha: String,
}

/// A landing found on some repository's main: the repository, when it reached main and why it was reworked.
#[derive(Clone, Debug, PartialEq)]
pub struct Judged {
    pub repo: String,
    pub at: i64,
    pub cause: Option<Cause>,
}

/// Where the history and the red-main culprits are read from: `<code_root>/<checkout>/main` and the
/// red-main-notify state directory, whose entries end in each red issue's first failing commit.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub code_root: Option<PathBuf>,
    pub culprits: Option<PathBuf>,
}

impl Sources {
    pub fn live() -> Self {
        // Unit tests pass their own sources so a developer's ~/code history never reaches a golden.
        if cfg!(test) {
            return Self::default();
        }
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let home = var("HOME");
        let state = var("XDG_STATE_HOME").or_else(|| home.as_ref().map(|home| home.join(".local/state")));
        Self {
            code_root: var("NORTH_CODE_ROOT").or_else(|| home.map(|home| home.join("code"))),
            culprits: var("RED_MAIN_STATE").or_else(|| state.map(|state| state.join("north/red-main-notify"))),
        }
    }
}

/// Every worker landing the ledger names that some main holds, by agent.
#[derive(Debug, Default)]
pub struct Rework {
    pub judged: BTreeMap<String, Judged>,
}

impl Rework {
    pub fn read(db: &Connection, sources: &Sources) -> rusqlite::Result<Self> {
        let Some(root) = sources.code_root.as_deref() else { return Ok(Self::default()) };
        let landings = landings(db)?;
        if landings.is_empty() {
            return Ok(Self::default());
        }
        let earliest: Option<String> = db.query_row(&format!("SELECT min(coalesce(landed_at, ended)) FROM runs WHERE {} AND landed_sha IS NOT NULL", worker_where()), [], |row| row.get(0))?;
        let since = earliest.and_then(|text| Timestamp::parse(&text).ok()).map(|at| at.unix_seconds() - 2 * 86_400);
        let histories = histories(root, since);
        let culprits = sources.culprits.as_deref().map(culprits).unwrap_or_default();
        Ok(Self { judged: judge(&landings, &histories, &culprits) })
    }

    pub fn reworked(&self) -> impl Iterator<Item = &String> {
        self.judged.iter().filter(|(_, judged)| judged.cause.is_some()).map(|(agent, _)| agent)
    }

    /// Reworked and judged landings per repository that reached main at or after `since`.
    pub fn rates(&self, since: i64) -> BTreeMap<String, (usize, usize)> {
        let mut rates: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for judged in self.judged.values().filter(|judged| judged.at >= since) {
            let entry = rates.entry(judged.repo.clone()).or_default();
            entry.0 += usize::from(judged.cause.is_some());
            entry.1 += 1;
        }
        rates
    }
}

/// The 7-day rework rate in percent above which `north health` fails.
pub fn alarm_pct() -> f64 {
    super::registry("rework_alarm_pct")
}

fn landings(db: &Connection) -> rusqlite::Result<Vec<Landing>> {
    let mut statement = db.prepare(&format!("SELECT agent, item, landed_sha FROM runs WHERE {} AND landed_sha IS NOT NULL AND landed_sha != ''", worker_where()))?;
    statement.query_map([], |row| Ok(Landing { agent: row.get(0)?, item: row.get(1)?, sha: row.get(2)? }))?.collect()
}

fn histories(root: &Path, since: Option<i64>) -> Vec<(String, Vec<Commit>)> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut found: Vec<(String, Vec<Commit>)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("main/.git").exists())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let commits = super::history::history(&entry.path().join("main"), since)?;
            Some((CATALOG.canonical(&name).to_owned(), commits))
        })
        .collect();
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn culprits(directory: &Path) -> BTreeSet<String> {
    let Ok(entries) = fs::read_dir(directory) else { return BTreeSet::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_string_lossy().rsplit('-').next().map(str::to_owned))
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .collect()
}

fn names_item(text: &str, item: &str) -> bool {
    text.match_indices(item).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + item.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_') && !after.is_some_and(|c| c.is_ascii_digit())
    })
}

struct Read<'a> {
    commit: &'a Commit,
    text: String,
    fix: bool,
    tokens: Vec<String>,
    reverts: Vec<&'a str>,
}

impl<'a> Read<'a> {
    fn new(commit: &'a Commit) -> Self {
        let text = format!("{}\n{}", commit.subject, commit.body);
        let lower = text.to_lowercase();
        let fix = text.contains("Follows:") || lower.contains("fixes") || lower.contains("fix forward");
        let tokens = if fix { SHA_TOKEN.find_iter(&text).map(|token| token.as_str().to_owned()).collect() } else { Vec::new() };
        let reverts = commit.body.match_indices(REVERTS).map(|(start, _)| &commit.body[start + REVERTS.len()..]).collect();
        Self { commit, text, fix, tokens, reverts }
    }

    fn names(&self, landing: &Landing) -> bool {
        self.tokens.iter().any(|token| landing.sha.starts_with(token.as_str())) || landing.item.as_deref().filter(|item| !item.is_empty()).is_some_and(|item| names_item(&self.text, item))
    }
}

const REVERTS: &str = "This reverts commit ";

fn cause(landed: &Commit, later: &[Read], landing: &Landing, culprits: &BTreeSet<String>) -> Option<Cause> {
    let window: Vec<&Read> = later.iter().filter(|read| read.commit.at <= landed.at + REWORK_WINDOW_S).collect();
    let revert_subject = format!("Revert \"{}\"", landed.subject);
    if window.iter().any(|read| read.commit.subject == revert_subject || read.reverts.iter().any(|rest| rest.starts_with(landed.sha.as_str()))) {
        return Some(Cause::Reverted);
    }
    let overlaps = |commit: &Commit| !landed.files.is_empty() && 2 * commit.files.iter().filter(|file| landed.files.contains(file)).count() >= landed.files.len();
    if window.iter().any(|read| (read.fix && read.names(landing)) || (read.commit.subject.starts_with("Fix") && overlaps(read.commit))) {
        return Some(Cause::FixedForward);
    }
    culprits.contains(&landed.sha).then_some(Cause::Culprit)
}

/// Each landing found in `histories` (oldest first per repository) with its rework cause, if any.
pub fn judge(landings: &[Landing], histories: &[(String, Vec<Commit>)], culprits: &BTreeSet<String>) -> BTreeMap<String, Judged> {
    let reads: Vec<(&str, Vec<Read>)> = histories.iter().map(|(repo, commits)| (repo.as_str(), commits.iter().map(Read::new).collect())).collect();
    let mut index: BTreeMap<&str, (&str, &[Read], usize)> = BTreeMap::new();
    for (repo, commits) in &reads {
        for (position, read) in commits.iter().enumerate() {
            index.insert(read.commit.sha.as_str(), (repo, commits.as_slice(), position));
        }
    }
    landings
        .iter()
        .filter_map(|landing| {
            let (repo, commits, position) = *index.get(landing.sha.as_str())?;
            let landed = commits[position].commit;
            let judged = Judged { repo: repo.to_owned(), at: landed.at, cause: cause(landed, &commits[position + 1..], landing, culprits) };
            Some((landing.agent.clone(), judged))
        })
        .collect()
}
