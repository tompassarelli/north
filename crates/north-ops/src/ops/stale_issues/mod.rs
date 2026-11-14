mod lanes;
mod owner;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const OLDER_THAN: i64 = 24 * 3600;

pub use lanes::Lanes;
pub use owner::Owners;

pub const COMMENT: &str = "Closed by the 24-hour rule (2026-11-14): no issue stays open past 24 hours. Rewrite the work that remains as issues that can each be finished within a day.";

const PROXY: &str = "proxy";

pub struct Stale {
    pub number: u64,
    pub created: i64,
    pub title: String,
    pub owner: Option<String>,
    pub open_boxes: Vec<String>,
    pub lanes: Vec<String>,
}

impl Stale {
    fn comment(&self, successor: Option<&str>) -> String {
        let mut text = COMMENT.to_owned();
        let named: Vec<&str> = self.open_boxes.iter().map(|text| text.lines().next().unwrap_or_default()).collect();
        let lanes: Vec<&str> = self.lanes.iter().map(String::as_str).collect();
        for (heading, lines) in [("Open boxes:", &named), ("Unlanded lanes whose Item: names this issue:", &lanes)] {
            if !lines.is_empty() {
                text.push_str(&format!("\n\n{heading}\n"));
                text.extend(lines.iter().map(|line| format!("- {line}\n")));
            }
        }
        if let Some(successor) = successor {
            text.push_str(&format!("\nRewritten as {successor}, which carries the open boxes.\n"));
        }
        text
    }

    fn successor_title(&self, repository: &str) -> String {
        let title = self.title.split(" (rewritten from ").next().unwrap_or(&self.title);
        format!("{title} (rewritten from {repository}#{})", self.number)
    }

    fn successor_body(&self, repository: &str) -> String {
        let mut body = format!("Rewritten from {repository}#{} by the 24-hour rule (2026-11-14); its open boxes follow.\n\n", self.number);
        if let Some(owner) = &self.owner {
            body.push_str(&format!("Owner: {owner}\n\n"));
        }
        body.extend(self.open_boxes.iter().map(|text| format!("- [ ] {text}\n")));
        body
    }
}

/// What the sweep does outside itself: messages, GitHub issues and the threads ledger's record of each rewrite.
pub trait Hands {
    fn message(&mut self, session: &str, text: &str) -> Result<(), String>;
    fn close(&mut self, number: u64, comment: &str) -> Result<(), String>;
    /// Files an issue in the swept repository and returns its number.
    fn file(&mut self, title: &str, body: &str) -> Result<u64, String>;
    /// The successor recorded for `closed`, `REPO#N`, by an earlier run.
    fn recorded_successor(&mut self, closed: &str) -> Result<Option<String>, String>;
    /// Records `successor` as the issue filed in place of `closed`, both `REPO#N`.
    fn record_successor(&mut self, closed: &str, successor: &str) -> Result<(), String>;
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

fn rewrite_request(reference: &str, successor: Option<&str>) -> String {
    match successor {
        Some(successor) => format!("{reference} closed at 24 h: its open boxes moved to {successor}, which your launches follow as they did {reference}; split it into issues finishable within a day if a box needs more, and re-point lanes that name {reference}"),
        None => format!("{reference} closed at 24 h: re-point lanes that name it"),
    }
}

/// The issue filed in place of `issue`, `REPO#N`, when it has open boxes: the one an earlier run recorded, else a new one carrying them, recorded before the close so a rerun files no second.
fn successor<H: Hands>(repository: &str, issue: &Stale, reference: &str, hands: &mut H) -> Result<Option<String>, String> {
    if issue.open_boxes.is_empty() {
        return Ok(None);
    }
    if let Some(recorded) = hands.recorded_successor(reference)? {
        return Ok(Some(recorded));
    }
    let filed = format!("{repository}#{}", hands.file(&issue.successor_title(repository), &issue.successor_body(repository))?);
    hands.record_successor(reference, &filed)?;
    Ok(Some(filed))
}

/// Closes each issue of `repository` created more than 24 hours before `now`, filing a successor for its open boxes and messaging its owner first and once.
pub fn sweep<H: Hands>(repository: &str, issues: &[Stale], now: i64, dry_run: bool, hands: &mut H, record: &mut Record) -> Outcome {
    let mut outcome = Outcome { lines: Vec::new(), failed: 0 };
    for issue in issues.iter().filter(|issue| now - issue.created > OLDER_THAN) {
        let reference = format!("{repository}#{}", issue.number);
        if dry_run {
            outcome.lines.push(format!("{reference}: would close; {} open boxes to a successor; owner {}", issue.open_boxes.len(), issue.owner.as_deref().unwrap_or(PROXY)));
            continue;
        }
        let mut step = || -> Result<String, String> {
            let successor = successor(repository, issue, &reference, hands)?;
            let recipient = issue.owner.as_deref().unwrap_or(PROXY);
            let told = if record.messaged.contains(&reference) {
                format!("{recipient} already messaged")
            } else {
                hands.message(recipient, &rewrite_request(&reference, successor.as_deref()))?;
                record.insert(&reference)?;
                format!("{recipient} messaged")
            };
            hands.close(issue.number, &issue.comment(successor.as_deref()))?;
            Ok(match successor {
                Some(successor) => format!("rewritten as {successor}; {told}"),
                None => told,
            })
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
