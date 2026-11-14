mod owner;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const OLDER_THAN: i64 = 24 * 3600;

pub use owner::Owners;

pub const COMMENT: &str = "Closed by the 24-hour rule (2026-11-14): no issue stays open past 24 hours. Rewrite the work that remains as issues that can each be finished within a day.";

pub struct Stale {
    pub number: u64,
    pub created: i64,
    pub owner: Option<String>,
}

pub trait Hands {
    fn message(&mut self, session: &str, text: &str) -> Result<(), String>;
    fn close(&mut self, number: u64, comment: &str) -> Result<(), String>;
}

/// The issues whose owner was messaged, one line each, so a run that fails after the message does not send it twice.
pub struct Record {
    file: PathBuf,
    messaged: BTreeSet<String>,
}

impl Record {
    pub fn open(file: &Path) -> Result<Self, String> {
        let messaged = match std::fs::read_to_string(file) {
            Ok(text) => text.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(error) => return Err(format!("{}: {error}", file.display())),
        };
        Ok(Self { file: file.to_path_buf(), messaged })
    }

    fn insert(&mut self, reference: &str) -> Result<(), String> {
        use std::io::Write;
        if let Some(directory) = self.file.parent().filter(|directory| !directory.as_os_str().is_empty()) {
            std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        }
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&self.file).map_err(|error| format!("{}: {error}", self.file.display()))?;
        writeln!(file, "{reference}").map_err(|error| format!("{}: {error}", self.file.display()))?;
        self.messaged.insert(reference.to_owned());
        Ok(())
    }
}

pub struct Outcome {
    pub lines: Vec<String>,
    pub failed: usize,
}

fn rewrite_request(reference: &str) -> String {
    format!("{reference} was closed by the 24-hour rule (2026-11-14). Rewrite the work that remains as issues that can each be finished within a day.")
}

/// Closes each issue of `repository` created more than 24 hours before `now`, messaging its owner first and once.
pub fn sweep<H: Hands>(repository: &str, issues: &[Stale], now: i64, dry_run: bool, hands: &mut H, record: &mut Record) -> Outcome {
    let mut outcome = Outcome { lines: Vec::new(), failed: 0 };
    for issue in issues.iter().filter(|issue| now - issue.created > OLDER_THAN) {
        let reference = format!("{repository}#{}", issue.number);
        if dry_run {
            outcome.lines.push(format!("{reference}: would close; owner {}", issue.owner.as_deref().unwrap_or("none")));
            continue;
        }
        let mut step = || -> Result<String, String> {
            let owner = match issue.owner.as_deref().filter(|_| !record.messaged.contains(&reference)) {
                Some(owner) => {
                    hands.message(owner, &rewrite_request(&reference))?;
                    record.insert(&reference)?;
                    format!("{owner} messaged")
                }
                None if issue.owner.is_some() => "owner already messaged".to_owned(),
                None => "no owner found".to_owned(),
            };
            hands.close(issue.number, COMMENT)?;
            Ok(owner)
        };
        match step() {
            Ok(owner) => outcome.lines.push(format!("{reference}: closed; {owner}")),
            Err(why) => {
                outcome.failed += 1;
                outcome.lines.push(format!("{reference}: not closed: {why}"));
            }
        }
    }
    outcome
}
