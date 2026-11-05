use super::snapshot::git;
use super::state::State;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

pub(super) const WINDOW_DAYS: i64 = 90;
pub(super) const WINDOW_COMMITS: usize = 2000;
const BULK_COMMIT_FILES: usize = 50;

pub(super) struct History {
    pub(super) commits: usize,
    pub(super) churn: Vec<(String, usize)>,
    changes: Vec<Vec<String>>,
}

fn tally(changes: &[Vec<String>]) -> Vec<(String, usize)> {
    let mut churn: Vec<(String, usize)> = Vec::new();
    let mut position: HashMap<&str, usize> = HashMap::new();
    for files in changes {
        for file in files {
            let at = *position.entry(file).or_insert_with(|| {
                churn.push((file.clone(), 0));
                churn.len() - 1
            });
            churn[at].1 += 1;
        }
    }
    churn
}

pub(super) fn history(repo: &Path, rev: &str) -> Result<History, String> {
    let at: i64 = git(repo, &["log", "-1", "--format=%ct", rev])?.trim().parse().map_err(|error| format!("commit time of {rev}: {error}"))?;
    let since = north_base::foundation::clock::Timestamp::from_unix_seconds(at - WINDOW_DAYS * 86_400).iso();
    let out = git(repo, &["log", &format!("-n{WINDOW_COMMITS}"), &format!("--since={since}"), "--no-renames", "--no-merges", "--format=%x1e", "--name-only", rev])?;
    let changes: Vec<Vec<String>> = out.split('\x1e').skip(1).map(|block| block.split('\n').filter(|line| !line.is_empty()).map(str::to_owned).collect()).collect();
    Ok(History { commits: changes.len(), churn: tally(&changes), changes })
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Pair {
    pub(super) a: String,
    pub(super) b: String,
    pub(super) support: usize,
    pub(super) confidence: f64,
}

pub(super) fn co_change(history: &History, keep: impl Fn(&str) -> bool, group: impl Fn(&str) -> Option<String>) -> Vec<Pair> {
    let mut counts: Vec<((String, String), usize)> = Vec::new();
    let mut position: HashMap<(String, String), usize> = HashMap::new();
    for files in &history.changes {
        let mut kept: Vec<&String> = files.iter().filter(|file| keep(file)).collect();
        if kept.len() < 2 || kept.len() > BULK_COMMIT_FILES {
            continue;
        }
        kept.sort();
        for (i, first) in kept.iter().enumerate() {
            for second in &kept[i + 1..] {
                let (ga, gb) = (group(first), group(second));
                if ga.is_some() && ga == gb {
                    continue;
                }
                let key = ((*first).clone(), (*second).clone());
                let at = *position.entry(key.clone()).or_insert_with(|| {
                    counts.push((key, 0));
                    counts.len() - 1
                });
                counts[at].1 += 1;
            }
        }
    }
    let churn: HashMap<&str, usize> = history.churn.iter().map(|(file, count)| (file.as_str(), *count)).collect();
    let mut pairs: Vec<Pair> = counts
        .into_iter()
        .filter(|(_, support)| *support >= 3)
        .map(|((a, b), support)| {
            let confidence = support as f64 / churn[a.as_str()].min(churn[b.as_str()]) as f64;
            Pair { a, b, support, confidence }
        })
        .collect();
    pairs.sort_by(|x, y| y.support.cmp(&x.support).then(y.confidence.total_cmp(&x.confidence)));
    pairs
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Allow {
    pub(super) rule: String,
    pub(super) path: String,
    pub(super) reason: String,
    pub(super) commit: String,
}

static ALLOW: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"^(\S+)\s+(\S+)\s*(?:—|--?)?\s*(.*)$").unwrap());

pub(super) fn allows(repo: &Path, range: &[&str]) -> Result<Vec<Allow>, String> {
    let mut args = vec!["log", "--format=%x1e%H%x1f%(trailers:key=Code-health-allow,valueonly,separator=%x1f)"];
    args.extend_from_slice(range);
    let out = git(repo, &args)?;
    let mut result = Vec::new();
    for block in out.split('\x1e').skip(1) {
        let mut values = block.trim().split('\x1f');
        let commit = values.next().unwrap_or_default().to_owned();
        for value in values {
            if let Some(found) = ALLOW.captures(value.trim()) {
                result.push(Allow { rule: found[1].to_owned(), path: found[2].to_owned(), reason: found[3].to_owned(), commit: commit.clone() });
            }
        }
    }
    Ok(result)
}

pub(super) fn churn_at(state: &State, repo: &Path, rev: &str) -> Result<Vec<(String, usize)>, String> {
    let sha = git(repo, &["rev-parse", &format!("{rev}^{{commit}}")])?.trim().to_owned();
    let directory = state.directory(repo).join("churn");
    let path = directory.join(format!("{sha}-{WINDOW_DAYS}-{WINDOW_COMMITS}.json"));
    if let Some(cached) = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()) {
        return Ok(cached);
    }
    let churn = history(repo, &sha)?.churn;
    std::fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    std::fs::write(&path, serde_json::to_string(&churn).map_err(|error| error.to_string())?).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(churn)
}
