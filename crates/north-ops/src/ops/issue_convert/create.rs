//! The creating half of `north issue convert --manifest FILE --sha256 HASH` (north#1137): files each reviewed row whose
//! box the parent body still holds as an issue through an injected tracker, ticks that box in the body, and records what
//! it made beside the manifest, so a second run creates nothing. Only `Github` reaches the network.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::boxes;
use super::manifest::Row;
use crate::foundation::clock::Timestamp;

/// The most issues one hour creates: GitHub allows more, and the run keeps headroom under its own limit.
const MOST_PER_HOUR: usize = 450;
const HOUR: i64 = 3600;
const FIRST_WAIT: i64 = 60;
const MOST_ATTEMPTS: u32 = 6;

/// Why a tracker call did not go through.
pub enum Refusal {
    /// GitHub turned the call away for its rate limits (HTTP 403 or 429); the same call is tried again after a wait.
    Limited,
    Failed(String),
}

pub struct NewIssue<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub kind: &'a str,
    pub labels: &'a [String],
}

/// Where converted issues are filed and the parent's body rewritten: GitHub, or a fake in the tests.
pub trait IssueTracker {
    fn create(&mut self, issue: &NewIssue) -> Result<u64, Refusal>;
    fn rewrite_body(&mut self, body: &str) -> Result<(), Refusal>;
    fn now(&self) -> i64;
    fn wait(&mut self, seconds: i64);
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Made {
    pub issue: u64,
    /// Unix seconds when the issue was created.
    pub at: i64,
}

/// The issues a conversion has made, keyed by the box text each one converted; saved beside the manifest as it grows.
pub struct Record {
    file: PathBuf,
    made: BTreeMap<String, Made>,
}

impl Record {
    pub fn open(file: &Path) -> Result<Self, String> {
        let made = match std::fs::read_to_string(file) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{} is not a record this command wrote ({error})", file.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(format!("{}: {error}", file.display())),
        };
        Ok(Self { file: file.to_path_buf(), made })
    }

    pub fn insert(&mut self, source: &str, made: Made) -> Result<(), String> {
        self.made.insert(source.to_owned(), made);
        let text = serde_json::to_string_pretty(&self.made).map_err(|error| error.to_string())?;
        std::fs::write(&self.file, text + "\n").map_err(|error| format!("{}: {error}", self.file.display()))
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Outcome {
    pub created: usize,
    pub rewritten: bool,
}

/// Files each row of `rows` that `record` does not hold yet, then ticks every recorded box in `body`. When that changes
/// the body, the body as it stood is saved under `state` and then rewritten.
pub fn convert<T: IssueTracker>(tracker: &mut T, body: &str, rows: &[Row], record: &mut Record, state: &Path, label: &str) -> Result<Outcome, String> {
    let mut window: Vec<i64> = record.made.values().map(|made| made.at).collect();
    let fresh: Vec<&Row> = rows.iter().filter(|row| row.verdict.created() && !record.made.contains_key(&row.source)).collect();
    for row in &fresh {
        pace(tracker, &mut window);
        let issue = NewIssue { title: &row.title, body: &row.body, kind: &row.kind, labels: &row.labels };
        let number = patiently(tracker, &row.title, |tracker| tracker.create(&issue))?;
        let at = tracker.now();
        window.push(at);
        record.insert(&row.source, Made { issue: number, at })?;
    }
    let mut next = body.to_owned();
    for (source, made) in &record.made {
        if let Some(ticked) = boxes::tick(&next, source, &format!("(converted to #{})", made.issue)) {
            next = ticked;
        }
    }
    if next == body {
        return Ok(Outcome { created: fresh.len(), rewritten: false });
    }
    std::fs::create_dir_all(state).map_err(|error| format!("{}: {error}", state.display()))?;
    let copy = state.join(format!("{label}-{}.md", tracker.now()));
    std::fs::write(&copy, body).map_err(|error| format!("{}: {error}", copy.display()))?;
    patiently(tracker, "the parent body", |tracker| tracker.rewrite_body(&next))?;
    Ok(Outcome { created: fresh.len(), rewritten: true })
}

/// Waits until one more creation keeps the hour under `MOST_PER_HOUR`, given the creation times in `window`.
fn pace<T: IssueTracker>(tracker: &mut T, window: &mut Vec<i64>) {
    loop {
        let now = tracker.now();
        window.retain(|at| *at > now - HOUR);
        match window.iter().min() {
            Some(oldest) if window.len() >= MOST_PER_HOUR => tracker.wait(oldest + HOUR - now),
            _ => return,
        }
    }
}

/// Makes one call, waiting out each rate-limit refusal before the same call is tried again.
fn patiently<T: IssueTracker, R>(tracker: &mut T, what: &str, mut call: impl FnMut(&mut T) -> Result<R, Refusal>) -> Result<R, String> {
    let mut wait = FIRST_WAIT;
    let mut attempt = 0;
    loop {
        attempt += 1;
        match call(tracker) {
            Ok(value) => return Ok(value),
            Err(Refusal::Failed(why)) => return Err(format!("{what}: {why}")),
            Err(Refusal::Limited) if attempt >= MOST_ATTEMPTS => return Err(format!("{what}: refused for rate limits {attempt} times in a row")),
            Err(Refusal::Limited) => {
                tracker.wait(wait);
                wait *= 2;
            }
        }
    }
}

/// GitHub through the gh CLI: files issues on `repo` and rewrites the body of issue `parent`.
pub struct Github {
    pub repo: String,
    pub parent: u64,
}

impl Github {
    fn send(&self, args: &[&str], payload: &Value) -> Result<Value, Refusal> {
        let mut child = Command::new("gh")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| Refusal::Failed(format!("cannot run gh: {error}")))?;
        let mut stdin = child.stdin.take().ok_or_else(|| Refusal::Failed("gh took no input".to_owned()))?;
        stdin.write_all(payload.to_string().as_bytes()).map_err(|error| Refusal::Failed(error.to_string()))?;
        drop(stdin);
        let out = child.wait_with_output().map_err(|error| Refusal::Failed(error.to_string()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(if err.contains("HTTP 403") || err.contains("HTTP 429") { Refusal::Limited } else { Refusal::Failed(err.trim().to_owned()) });
        }
        serde_json::from_slice(&out.stdout).map_err(|error| Refusal::Failed(error.to_string()))
    }
}

impl IssueTracker for Github {
    fn create(&mut self, issue: &NewIssue) -> Result<u64, Refusal> {
        let payload = json!({ "title": issue.title, "body": issue.body, "type": issue.kind, "labels": issue.labels });
        let endpoint = format!("repos/{}/issues", self.repo);
        let answer = self.send(&["api", "--method", "POST", endpoint.as_str(), "--input", "-"], &payload)?;
        answer["number"].as_u64().ok_or_else(|| Refusal::Failed("GitHub's answer holds no issue number".to_owned()))
    }

    fn rewrite_body(&mut self, body: &str) -> Result<(), Refusal> {
        let endpoint = format!("repos/{}/issues/{}", self.repo, self.parent);
        self.send(&["api", "--method", "PATCH", endpoint.as_str(), "--input", "-"], &json!({ "body": body })).map(drop)
    }

    fn now(&self) -> i64 {
        Timestamp::now().unix_seconds()
    }

    fn wait(&mut self, seconds: i64) {
        std::thread::sleep(Duration::from_secs(seconds.max(0) as u64));
    }
}
