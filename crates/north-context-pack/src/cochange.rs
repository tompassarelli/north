use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

const DAYS: &str = "90";
const MOST_FILES: usize = 4;
const REVERT_WINDOW_SECONDS: i64 = 48 * 3600;

static REVERTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)reverts?:? (?:commit )?([0-9a-f]{7,40})").expect("revert pattern"));

#[derive(Debug, PartialEq, Eq)]
struct Commit {
    sha: String,
    time: i64,
    subject: String,
    body: String,
    files: Vec<String>,
}

pub struct History {
    pub with: Vec<(String, usize)>,
    pub reverts: Vec<String>,
}

pub fn of(root: &Path, path: &str) -> History {
    let touching = git(root, &["log", &format!("--since={DAYS} days ago"), "--format=%H", "--", path], "");
    let log = git(root, &["log", "--no-walk=unsorted", "--stdin", "--no-renames", "--format=%x01%H%x1f%ct%x1f%s%x1f%b%x02", "--name-only"], &touching);
    let commits = parse(&log);
    let reverts = quick_reverts(root, &commits);
    History { with: most_with(&commits, path), reverts }
}

fn most_with(commits: &[Commit], path: &str) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for file in commits.iter().flat_map(|commit| &commit.files).filter(|file| *file != path) {
        *counts.entry(file).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().map(|(file, count)| (file.to_owned(), count)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(MOST_FILES);
    ranked
}

fn quick_reverts(root: &Path, commits: &[Commit]) -> Vec<String> {
    let reverting: Vec<(&Commit, String)> = commits.iter().filter(|commit| commit.subject.starts_with("Revert")).filter_map(|commit| Some((commit, REVERTED.captures(&commit.body)?[1].to_owned()))).collect();
    if reverting.is_empty() {
        return Vec::new();
    }
    let mut arguments = vec!["show", "-s", "--format=%h %ct"];
    arguments.extend(reverting.iter().map(|(_, target)| target.as_str()));
    let times = git(root, &arguments, "");
    reverting
        .iter()
        .zip(times.lines())
        .filter_map(|((commit, target), line)| {
            let target_time: i64 = line.split_whitespace().nth(1)?.parse().ok()?;
            (commit.time - target_time <= REVERT_WINDOW_SECONDS).then(|| format!("{} reverted {target:.10} within 48 h", &commit.sha[..10.min(commit.sha.len())]))
        })
        .collect()
}

fn parse(log: &str) -> Vec<Commit> {
    log.split('\x01')
        .filter(|record| !record.trim().is_empty())
        .filter_map(|record| {
            let (header, names) = record.split_once('\x02')?;
            let mut fields = header.splitn(4, '\x1f');
            let (sha, time, subject, body) = (fields.next()?, fields.next()?, fields.next()?, fields.next()?);
            Some(Commit { sha: sha.to_owned(), time: time.parse().ok()?, subject: subject.to_owned(), body: body.to_owned(), files: names.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect() })
        })
        .collect()
}

fn git(root: &Path, arguments: &[&str], stdin: &str) -> String {
    let spawned = Command::new("git").arg("-C").arg(root).args(arguments).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = spawned else { return String::new() };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin.as_bytes());
    }
    child.wait_with_output().map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default()
}
