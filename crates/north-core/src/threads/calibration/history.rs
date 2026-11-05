//! First-parent history of a checkout's origin/main, read incrementally and shared between callers.
//!
//! A full `git log --name-only` over a large main takes minutes, and every tier pick and ledger summary needs it,
//! so the parsed commits live in a cache file in the checkout's git directory, keyed by the origin/main sha they
//! end at. A call at the same sha starts no `git log`; a call after main moved reads only `<cached>..<head>`; and
//! concurrent callers queue on one file lock, so only the first of them reads anything.
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::rework::Commit;
use crate::foundation::clock::Timestamp;

const FORMAT: &str = "--format=%x1e%H%x1f%ct%x1f%s%x1f%b%x1f";

/// Runs git in a checkout and returns its stdout when it exits 0.
pub trait Git {
    fn run(&self, checkout: &Path, args: &[&str]) -> Option<String>;
}

/// The `git` on PATH.
pub struct SystemGit;

impl Git for SystemGit {
    fn run(&self, checkout: &Path, args: &[&str]) -> Option<String> {
        let output = Command::new("git").arg("-C").arg(checkout).args(args).output().ok().filter(|output| output.status.success())?;
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// The commits read so far: every first-parent commit up to `head` that a `git log --since=<since>` returns.
#[derive(Serialize, Deserialize)]
struct Cache {
    head: String,
    since: Option<i64>,
    commits: Vec<Commit>,
}

impl Cache {
    /// Whether these commits hold everything a read from `since` returns.
    fn covers(&self, since: Option<i64>) -> bool {
        match (self.since, since) {
            (None, _) => true,
            (Some(cached), Some(wanted)) => cached <= wanted,
            (Some(_), None) => false,
        }
    }
}

/// First-parent history of a checkout's origin/main from `since`, oldest first, as `git log --since` gives it.
pub fn history(checkout: &Path, since: Option<i64>) -> Option<Vec<Commit>> {
    history_with(&SystemGit, checkout, since)
}

pub fn history_with(git: &impl Git, checkout: &Path, since: Option<i64>) -> Option<Vec<Commit>> {
    let head = git.run(checkout, &["rev-parse", "--verify", "--quiet", "origin/main^{commit}"])?.trim().to_owned();
    let Some((path, _lock)) = git_dir(checkout).and_then(|directory| lock(&directory)) else {
        return log(git, checkout, &head, since).map(|commits| from(commits, since));
    };
    let cached = read(&path).filter(|cache| cache.covers(since));
    let cache = match cached {
        Some(cache) if cache.head == head => cache,
        Some(mut cache) if git.run(checkout, &["merge-base", "--is-ancestor", &cache.head, &head]).is_some() => {
            cache.commits.extend(log(git, checkout, &format!("{}..{head}", cache.head), None)?);
            let commits = from(cache.commits, cache.since);
            store(&path, Cache { head, since: cache.since, commits })
        }
        _ => {
            let commits = log(git, checkout, &head, since)?;
            store(&path, Cache { head, since, commits })
        }
    };
    Some(from(cache.commits, since))
}

/// The commits `git log --since` keeps from an oldest-first first-parent walk: it stops at the newest commit older
/// than `since`, so those after it.
fn from(mut commits: Vec<Commit>, since: Option<i64>) -> Vec<Commit> {
    if let Some(since) = since
        && let Some(stop) = commits.iter().rposition(|commit| commit.at < since)
    {
        commits.drain(..=stop);
    }
    commits
}

fn log(git: &impl Git, checkout: &Path, range: &str, since: Option<i64>) -> Option<Vec<Commit>> {
    let since = since.map(|since| format!("--since={}", Timestamp::from_unix_seconds(since).iso()));
    let mut args = vec!["log", "--first-parent", "--reverse", FORMAT, "--name-only"];
    args.extend(since.as_deref());
    args.extend([range, "--"]);
    Some(parse(&git.run(checkout, &args)?))
}

fn parse(text: &str) -> Vec<Commit> {
    text.split('\u{1e}')
        .filter_map(|record| {
            let mut fields = record.splitn(5, '\u{1f}');
            let sha = fields.next()?.trim().to_owned();
            let at = fields.next()?.trim().parse().ok()?;
            let subject = fields.next()?.to_owned();
            let body = fields.next()?.to_owned();
            let files = fields.next().unwrap_or_default().lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect();
            Some(Commit { sha, at, subject, body, files })
        })
        .collect()
}

/// The checkout's own git directory: `.git`, or where a `gitdir:` file in its place points.
fn git_dir(checkout: &Path) -> Option<PathBuf> {
    let dot_git = checkout.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let pointer = fs::read_to_string(&dot_git).ok()?;
    let target = Path::new(pointer.strip_prefix("gitdir:")?.trim());
    Some(checkout.join(target))
}

/// The cache path once this process holds the history lock, which it keeps until the returned file drops.
fn lock(directory: &Path) -> Option<(PathBuf, File)> {
    let file = File::options().create(true).truncate(false).write(true).open(directory.join("north-history.lock")).ok()?;
    file.lock().ok()?;
    Some((directory.join("north-history.json"), file))
}

fn read(path: &Path) -> Option<Cache> {
    serde_json::from_reader(BufReader::new(File::open(path).ok()?)).ok()
}

/// Writes `cache` atomically, so a reader without the lock never sees half of it; a failed write only costs the
/// next caller a fresh read.
fn store(path: &Path, cache: Cache) -> Cache {
    let written = path.parent().and_then(|directory| tempfile::NamedTempFile::new_in(directory).ok()).and_then(|file| {
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &cache).ok()?;
        writer.flush().ok()?;
        writer.into_inner().ok()?.persist(path).ok()
    });
    if written.is_none() {
        let _ = fs::remove_file(path);
    }
    cache
}
