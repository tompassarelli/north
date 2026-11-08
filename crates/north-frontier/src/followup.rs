use std::collections::BTreeMap;
use std::path::Path;

use north_base::error::NorthError;
use north_providers::ledger::Ledger;
use north_providers::router::pricing::{PriceTable, TokenUse};

use crate::ab::{AbRow, LongTerm, Usage};
use crate::sandbox::git_text;

pub const WINDOW_SECONDS: i64 = 14 * 86_400;

/// The lines a diff removes or replaces, as (file, first line, count) on its old side.
fn removed(diff: &str) -> Vec<(String, usize, usize)> {
    let mut found = Vec::new();
    let mut file = String::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("--- ") {
            file = path.strip_prefix("a/").unwrap_or("").to_owned();
        } else if let Some(header) = line.strip_prefix("@@ -") {
            let old = header.split(' ').next().unwrap_or("");
            let (start, count) = old.split_once(',').unwrap_or((old, "1"));
            let (start, count): (usize, usize) = (start.parse().unwrap_or(0), count.parse().unwrap_or(0));
            if count > 0 && !file.is_empty() {
                found.push((file.clone(), start, count));
            }
        }
    }
    found
}

fn changed_files(dir: &Path, sha: &str) -> Result<Vec<String>, NorthError> {
    let text = git_text(dir, &["diff", "--name-only", "--no-renames", &format!("{sha}^"), sha])?;
    Ok(text.lines().map(str::to_owned).collect())
}

fn blames_to(dir: &Path, sha: &str, before: &str, file: &str, start: usize, count: usize) -> bool {
    let span = format!("{start},{}", start + count - 1);
    let prefix = format!("{sha} ");
    git_text(dir, &["blame", "--line-porcelain", "-L", &span, before, "--", file]).is_ok_and(|text| text.lines().any(|line| line.starts_with(&prefix)))
}

/// The commits on `reference` landed within 14 days after `sha` whose removed or replaced lines were, just before them, lines that `sha` added.
pub fn changing_commits(dir: &Path, reference: &str, sha: &str, landed_at: i64) -> Result<Vec<String>, NorthError> {
    let files = changed_files(dir, sha)?;
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut log = vec!["log", "--format=%H %ct", "--no-merges", "--no-renames"];
    let range = format!("{sha}..{reference}");
    log.push(&range);
    log.push("--");
    log.extend(files.iter().map(String::as_str));
    let mut found = Vec::new();
    for entry in git_text(dir, &log)?.lines() {
        let Some((commit, time)) = entry.split_once(' ') else { continue };
        if time.parse::<i64>().map_or(true, |time| time <= landed_at || time > landed_at + WINDOW_SECONDS) {
            continue;
        }
        let mut diff = vec!["diff", "--unified=0", "--no-color", "--no-ext-diff", "--no-renames"];
        let (before, after) = (format!("{commit}^"), commit.to_owned());
        diff.extend([before.as_str(), after.as_str(), "--"]);
        diff.extend(files.iter().map(String::as_str));
        let hit = removed(&git_text(dir, &diff)?).iter().any(|(file, start, count)| blames_to(dir, sha, &before, file, *start, *count));
        if hit {
            found.push(commit.to_owned());
        }
    }
    Ok(found)
}

/// What each landed commit cost in the ledger: tokens and API-equivalent dollars of every run of the task that landed it.
pub struct Spend {
    by_sha: BTreeMap<String, Usage>,
}

impl Spend {
    pub fn read(ledger: &Ledger) -> Result<Self, NorthError> {
        let prices = PriceTable::load().map_err(NorthError::Json)?;
        let mut by_task: BTreeMap<String, Usage> = BTreeMap::new();
        for run in ledger.runs()? {
            let Some(task) = run.task_ref else { continue };
            let usage = by_task.entry(task).or_default();
            usage.tokens_in += run.tokens_in.max(0) as u64;
            usage.tokens_out += run.tokens_out.max(0) as u64;
            let tokens = TokenUse { input: run.tokens_in, output: run.tokens_out, cache_read: run.tokens_cache_read, cache_write: run.tokens_cache_write };
            usage.cost_usd += run.model.as_deref().and_then(|model| prices.price(model)).map_or(0.0, |price| price.of(tokens));
        }
        let mut by_sha: BTreeMap<String, Usage> = BTreeMap::new();
        for task in ledger.tasks()? {
            if let (Some(sha), Some(usage)) = (task.landed_sha, by_task.get(&task.task_ref)) {
                by_sha.entry(sha).or_default().add(usage);
            }
        }
        Ok(Self { by_sha })
    }

    #[cfg(test)]
    pub fn of(by_sha: BTreeMap<String, Usage>) -> Self {
        Self { by_sha }
    }

    fn usage(&self, sha: &str) -> Usage {
        self.by_sha.get(sha).cloned().unwrap_or_default()
    }
}

/// The long-term usage of every landed row whose 14 days have passed at `now`; a row git cannot read is left out and named on stderr.
pub fn long_term(dir: &Path, reference: &str, spend: &Spend, rows: &[AbRow], now: i64) -> LongTerm {
    let mut found = LongTerm::new();
    for row in rows {
        let (Some(sha), Some(landed_at)) = (row.landed_sha.as_deref(), row.landed_at) else { continue };
        if landed_at + WINDOW_SECONDS > now {
            continue;
        }
        match changing_commits(dir, reference, sha, landed_at) {
            Ok(commits) => {
                let mut total = Usage::default();
                commits.iter().for_each(|commit| total.add(&spend.usage(commit)));
                found.insert(row.task.clone(), total);
            }
            Err(error) => eprintln!("north test frontier: box {} has no long-term usage: {error}", row.task),
        }
    }
    found
}
