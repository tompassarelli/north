//! Filing a detector finding on the issue that owns its cause: one box per key, and a later episode whose cause
//! already has an open box comments its new reading on that issue instead of filing a second box.
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::PathBuf;

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};

use super::Finding;
use crate::flow::{Host, Places, write_atomic};

const FILE_LIMIT: usize = 12;
const STATE: &str = "detectors.json";
const LOCK: &str = "detectors.lock";

/// What one detector finding did to its owning issue: filed its box, commented a repeated cause that an open box
/// already covers, or left the issue alone.
#[derive(Debug, Eq, PartialEq)]
enum Appended {
    Box,
    Commented,
    Skipped(&'static str),
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct State {
    filed: BTreeMap<String, Filed>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Filed {
    at: f64,
    issue: String,
}

pub struct Outcome {
    pub finding: Finding,
    pub filed: bool,
    pub note: String,
}

impl Outcome {
    pub fn line(&self) -> String {
        format!("detectors: {} {}: {}", self.finding.detector.name(), self.finding.key, self.note)
    }
}

pub struct Filer<'a> {
    places: &'a Places,
    host: &'a dyn Host,
}

fn issue_of(owner: &str) -> Option<(String, String)> {
    let (repo, number) = owner.rsplit_once('#')?;
    let repo = repo.rsplit('/').next().filter(|repo| !repo.is_empty())?;
    number.bytes().all(|byte| byte.is_ascii_digit()).then(|| (format!("{}/{repo}", crate::dispatch::OWNER), number.to_owned()))
}

/// Whether the issue body already carries an unticked box for this cause, which a later episode's reading belongs
/// under as a comment: a detector key names one episode, but its title names the cause every episode shares.
fn open_box(body: &str, title: &str) -> bool {
    let line = format!("- [ ] **{title}.**");
    body.lines().any(|text| text.trim_start().starts_with(&line))
}

fn repeat_comment(finding: &Finding) -> String {
    format!(
        "**{} saw this cause again.** {}\n\nThe open box \"{}\" above covers the same cause, so this tick commented the new reading here instead of filing a second box.\n\n(detector {})",
        finding.detector.name(),
        finding.detail,
        finding.title,
        finding.key
    )
}

impl<'a> Filer<'a> {
    pub fn new(places: &'a Places, host: &'a dyn Host) -> Self {
        Self { places, host }
    }

    fn path(&self) -> PathBuf {
        self.places.flow().join(STATE)
    }

    fn load(&self) -> State {
        std::fs::read_to_string(self.path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn save(&self, state: &State) -> Result<(), String> {
        write_atomic(&self.path(), &(serde_json::to_string(state).map_err(|error| error.to_string())? + "\n")).map_err(|error| error.to_string())
    }

    fn append(&self, finding: &Finding, slug: &str, number: &str) -> Result<Appended, String> {
        let issue = self.host.json(&["gh", "issue", "view", number, "-R", slug, "--json", "body,state"]).ok_or_else(|| format!("gh issue view {slug}#{number} did not answer"))?;
        if issue["state"].as_str() != Some("OPEN") {
            return Ok(Appended::Skipped("the owning issue is not open"));
        }
        let body = issue["body"].as_str().unwrap_or_default();
        if body.contains(&format!("(detector {})", finding.key)) {
            return Ok(Appended::Skipped("the owning issue already carries the box"));
        }
        if open_box(body, &finding.title) {
            let comment = repeat_comment(finding);
            self.host.run(&["gh", "issue", "comment", number, "-R", slug, "--body-file", "-"], None, Some(&comment)).ok_or_else(|| format!("gh issue comment {slug}#{number} failed"))?;
            return Ok(Appended::Commented);
        }
        let edited = format!("{}\n{}\n", body.trim_end(), finding.box_line());
        self.host.run(&["gh", "issue", "edit", number, "-R", slug, "--body-file", "-"], None, Some(&edited)).ok_or_else(|| format!("gh issue edit {slug}#{number} failed"))?;
        Ok(Appended::Box)
    }

    pub fn filed_keys(&self) -> std::collections::BTreeSet<String> {
        self.load().filed.into_keys().collect()
    }

    pub fn file(&self, findings: &[Finding], now: f64) -> Result<Vec<Outcome>, String> {
        let directory = self.places.flow();
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let lock = OpenOptions::new().create(true).append(true).open(directory.join(LOCK)).map_err(|error| error.to_string())?;
        flock(&lock, FlockOperation::LockExclusive).map_err(|error| error.to_string())?;
        let mut state = self.load();
        let mut outcomes = Vec::new();
        let mut written = 0;
        for finding in findings {
            let mut outcome = |filed: bool, note: String| outcomes.push(Outcome { finding: finding.clone(), filed, note });
            if state.filed.contains_key(&finding.key) {
                outcome(false, "already filed".into());
                continue;
            }
            let Some(owner) = finding.owner.as_deref() else {
                outcome(false, "not filed: the source names no owning issue".into());
                continue;
            };
            let Some((slug, number)) = issue_of(owner) else {
                outcome(false, format!("not filed: owner {owner} is not REPO#N"));
                continue;
            };
            if written >= FILE_LIMIT {
                outcome(false, "deferred: this tick filed its limit".into());
                continue;
            }
            match self.append(finding, &slug, &number) {
                Ok(appended) => {
                    state.filed.insert(finding.key.clone(), Filed { at: now, issue: owner.to_owned() });
                    self.save(&state)?;
                    let note = match appended {
                        Appended::Box => {
                            written += 1;
                            format!("filed a box on {owner}")
                        }
                        Appended::Commented => format!("commented the repeated cause on {owner} instead of filing a second box"),
                        Appended::Skipped(why) => format!("not filed: {why}"),
                    };
                    outcome(appended == Appended::Box, note);
                }
                Err(error) => outcome(false, format!("not filed: {error}")),
            }
        }
        self.save(&state)?;
        Ok(outcomes)
    }
}

#[cfg(test)]
#[path = "filing_tests.rs"]
mod tests;
