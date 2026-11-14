//! Filing a detector finding on the issue that owns its cause, or the daily turn as an issue of its own: one box per key, and a later episode whose cause
//! already has an open box comments its new reading on that issue instead of filing a second box.
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::PathBuf;

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};

use super::{Finding, waits};
use crate::flow::{Host, Places, write_atomic};

const FILE_LIMIT: usize = 12;
const STATE: &str = "detectors.json";
const LOCK: &str = "detectors.lock";
const OWN_REPO: &str = "north";

/// What one detector finding did to its owning issue: filed its box, commented a repeated cause that an open box
/// already covers, reopened a closed owner that its box must go on, or left the issue alone.
#[derive(Debug, Eq, PartialEq)]
enum Appended {
    Box,
    Commented,
    Reopened,
    Skipped(&'static str),
}

/// Where a finding's box goes: on the issue that owns its cause, or, for a detector that names no standing owner, an
/// issue of its own.
enum Home<'f> {
    Owner { owner: &'f str, slug: String, number: String },
    Own,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct State {
    filed: BTreeMap<String, Filed>,
    #[serde(default)]
    subjects: BTreeMap<String, Subject>,
}

/// The newest box a detector filed about one subject: when, under which key and issue, and the wait it was filed
/// after, which the next box's wait is learned from.
#[derive(Debug, Deserialize, Serialize)]
struct Subject {
    at: f64,
    key: String,
    issue: String,
    wait_s: f64,
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

fn subject_id(finding: &Finding) -> String {
    format!("{}:{}", finding.detector.name(), finding.subject())
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

fn reopen_comment(finding: &Finding) -> String {
    format!(
        "**{} reopened this issue.** {}\n\nThe issue was closed, so the box \"{}\" had no open owner to land on; the next detector tick files it here.\n\n(detector {})",
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

    /// Whether the box a subject's last finding filed is ticked, and if so whether its line names a commit.
    fn closed(&self, last: &Subject) -> Result<Option<bool>, String> {
        let (slug, number) = issue_of(&last.issue).ok_or_else(|| format!("owner {} is not REPO#N", last.issue))?;
        let issue = self.host.json(&["gh", "issue", "view", &number, "-R", &slug, "--json", "body"]).ok_or_else(|| format!("gh issue view {slug}#{number} did not answer"))?;
        let marker = format!("(detector {})", last.key);
        let line = issue["body"].as_str().unwrap_or_default().lines().find(|text| text.contains(&marker));
        Ok(line.filter(|text| text.trim_start().starts_with("- [x]")).map(waits::names_commit))
    }

    /// The wait that applies before a finding's subject gets another box, and the time its last box was filed when
    /// that wait has not passed yet.
    fn wait(&self, state: &State, finding: &Finding, policy: &waits::Policy, now: f64) -> Result<(f64, Option<f64>), String> {
        let Some(last) = state.subjects.get(&subject_id(finding)) else {
            return Ok((policy.floor_s, None));
        };
        match self.closed(last)? {
            None => Ok((last.wait_s, None)),
            Some(changed) => {
                let wait = waits::after(policy, last.wait_s, changed);
                Ok((wait, (now < last.at + wait).then_some(last.at)))
            }
        }
    }

    fn append(&self, finding: &Finding, slug: &str, number: &str) -> Result<Appended, String> {
        let issue = self.host.json(&["gh", "issue", "view", number, "-R", slug, "--json", "body,state"]).ok_or_else(|| format!("gh issue view {slug}#{number} did not answer"))?;
        if issue["state"].as_str() != Some("OPEN") {
            let comment = reopen_comment(finding);
            self.host.run(&["gh", "issue", "reopen", number, "-R", slug, "--comment", &comment], None, None).ok_or_else(|| format!("gh issue reopen {slug}#{number} failed"))?;
            return Ok(Appended::Reopened);
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

    /// Files a finding as an issue of its own holding its one box, which the landing closes once that box is ticked; while
    /// an issue of the same title is open, its new reading is commented there instead, so the issues never stack.
    fn open_own(&self, finding: &Finding) -> Result<(Appended, String), String> {
        let slug = format!("{}/{OWN_REPO}", crate::dispatch::OWNER);
        let search = format!("\"{}\" in:title", finding.title);
        let listed = self.host.json(&["gh", "issue", "list", "-R", &slug, "--state", "open", "--search", &search, "--json", "number,title"]).ok_or_else(|| format!("gh issue list {slug} did not answer"))?;
        let open = listed.as_array().into_iter().flatten().find(|issue| issue["title"].as_str() == Some(finding.title.as_str())).and_then(|issue| issue["number"].as_u64());
        if let Some(number) = open {
            let number = number.to_string();
            self.host.run(&["gh", "issue", "comment", &number, "-R", &slug, "--body-file", "-"], None, Some(&repeat_comment(finding))).ok_or_else(|| format!("gh issue comment {slug}#{number} failed"))?;
            return Ok((Appended::Commented, format!("{OWN_REPO}#{number}")));
        }
        let created = self.host.run(&["gh", "issue", "create", "-R", &slug, "--label", "enhancement", "--title", &finding.title, "--body-file", "-"], None, Some(&format!("{}\n", finding.box_line()))).ok_or_else(|| format!("gh issue create {slug} failed"))?;
        let number = created.trim().rsplit('/').next().filter(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).ok_or_else(|| format!("gh issue create printed no issue URL: {}", created.trim()))?;
        Ok((Appended::Box, format!("{OWN_REPO}#{number}")))
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
            let (filed, note) = self.file_one(&mut state, finding, now, &mut written)?;
            outcomes.push(Outcome { finding: finding.clone(), filed, note });
        }
        self.save(&state)?;
        Ok(outcomes)
    }

    /// Files one finding: appends its box or comments its repeated cause on the owner, and records it in
    /// `state`; `written` counts this tick's boxes so one tick cannot exceed its filing limit.
    fn file_one(&self, state: &mut State, finding: &Finding, now: f64, written: &mut usize) -> Result<(bool, String), String> {
        if state.filed.contains_key(&finding.key) {
            return Ok((false, "already filed".into()));
        }
        let home = match finding.owner.as_deref() {
            Some(owner) => match issue_of(owner) {
                Some((slug, number)) => Home::Owner { owner, slug, number },
                None => return Ok((false, format!("not filed: owner {owner} is not REPO#N"))),
            },
            None if finding.detector.files_own_issue() => Home::Own,
            None => return Ok((false, "not filed: the source names no owning issue".into())),
        };
        if *written >= FILE_LIMIT {
            return Ok((false, "deferred: this tick filed its limit".into()));
        }
        let policy = waits::policy(finding.detector);
        let wait = match policy.as_ref().map(|policy| self.wait(state, finding, policy, now)).transpose() {
            Ok(wait) => wait,
            Err(error) => return Ok((false, format!("not filed: {error}"))),
        };
        if let Some((wait, Some(last))) = wait {
            return Ok((false, format!("waiting: the last box on {} was filed {} ago and this subject now waits {} between boxes", finding.subject(), waits::describe(now - last), waits::describe(wait))));
        }
        let appended = match &home {
            Home::Owner { owner, slug, number } => self.append(finding, slug, number).map(|appended| (appended, (*owner).to_owned())),
            Home::Own => self.open_own(finding),
        };
        match appended {
            Ok((Appended::Reopened, owner)) => Ok((false, format!("reopened {owner}: its box is filed on the next tick"))),
            Ok((appended, owner)) => {
                state.filed.insert(finding.key.clone(), Filed { at: now, issue: owner.clone() });
                if let (Appended::Box, Some((wait_s, _))) = (&appended, wait) {
                    state.subjects.insert(subject_id(finding), Subject { at: now, key: finding.key.clone(), issue: owner.clone(), wait_s });
                }
                self.save(state)?;
                let note = match appended {
                    Appended::Box => {
                        *written += 1;
                        if matches!(home, Home::Own) { format!("filed its own issue {owner}") } else { format!("filed a box on {owner}") }
                    }
                    Appended::Commented => format!("commented the repeated cause on {owner} instead of filing a second box"),
                    Appended::Reopened => unreachable!("handled before the key is recorded"),
                    Appended::Skipped(why) => format!("not filed: {why}"),
                };
                Ok((appended == Appended::Box, note))
            }
            Err(error) => Ok((false, format!("not filed: {error}"))),
        }
    }
}

#[cfg(test)]
#[path = "filing_tests.rs"]
mod tests;
