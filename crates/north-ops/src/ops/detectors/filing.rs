use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::PathBuf;

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};

use super::Finding;
use crate::ops::flow::{Host, Places, write_atomic};

const FILE_LIMIT: usize = 12;
const STATE: &str = "detectors.json";
const LOCK: &str = "detectors.lock";

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

    fn append(&self, finding: &Finding, slug: &str, number: &str) -> Result<Option<&'static str>, String> {
        let issue = self.host.json(&["gh", "issue", "view", number, "-R", slug, "--json", "body,state"]).ok_or_else(|| format!("gh issue view {slug}#{number} did not answer"))?;
        if issue["state"].as_str() != Some("OPEN") {
            return Ok(Some("the owning issue is not open"));
        }
        let body = issue["body"].as_str().unwrap_or_default();
        if body.contains(&format!("(detector {})", finding.key)) {
            return Ok(Some("the owning issue already carries the box"));
        }
        let edited = format!("{}\n{}\n", body.trim_end(), finding.box_line());
        self.host.run(&["gh", "issue", "edit", number, "-R", slug, "--body-file", "-"], None, Some(&edited)).ok_or_else(|| format!("gh issue edit {slug}#{number} failed"))?;
        Ok(None)
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
                Ok(skipped) => {
                    state.filed.insert(finding.key.clone(), Filed { at: now, issue: owner.to_owned() });
                    self.save(&state)?;
                    written += usize::from(skipped.is_none());
                    outcome(skipped.is_none(), skipped.map_or_else(|| format!("filed a box on {owner}"), |why| format!("not filed: {why}")));
                }
                Err(error) => outcome(false, format!("not filed: {error}")),
            }
        }
        self.save(&state)?;
        Ok(outcomes)
    }
}
