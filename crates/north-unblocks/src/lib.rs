//! The coordinator's manual unblocks (north#1577): each trip clear, relaunch after a launch refusal, host unit restart and
//! hand-carried patch is kept with its class, and the second of one class within a day is refused unless it names an open
//! class-fix issue that has an owner, so a cause repeated by hand turns into an owned fix instead of an eighth clear.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod review;

const FILE: &str = "north/unblocks.jsonl";
const REFUSALS: &str = "north/launch-refusals.jsonl";
pub const WINDOW_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    TripClear,
    Relaunch,
    HostRestart,
    PatchCarry,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::TripClear => "trip clear",
            Self::Relaunch => "relaunch after a launch refusal",
            Self::HostRestart => "host unit restart",
            Self::PatchCarry => "hand-carried patch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub at: u64,
    pub kind: Kind,
    pub class: String,
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
}

/// Whether an issue is open and has an owner; the live answer asks GitHub, the tests give their own.
pub trait Issues {
    fn owned_open(&self, issue: &IssueName) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueName {
    pub repo: String,
    pub number: u64,
}

impl IssueName {
    /// `REPO#N` or `OWNER/REPO#N`; a bare repository is tompassarelli's.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (repo, number) = text.split_once('#').ok_or_else(|| format!("{text} is no issue; give REPO#N such as north#1577"))?;
        let number = number.parse::<u64>().map_err(|_| format!("{text} is no issue; give REPO#N such as north#1577"))?;
        if repo.is_empty() || repo.split('/').count() > 2 {
            return Err(format!("{text} is no issue; give REPO#N such as north#1577"));
        }
        let repo = if repo.contains('/') { repo.to_owned() } else { format!("tompassarelli/{repo}") };
        Ok(Self { repo, number })
    }

    pub fn label(&self) -> String {
        format!("{}#{}", self.repo.strip_prefix("tompassarelli/").unwrap_or(&self.repo), self.number)
    }
}

/// GitHub's own copy of an issue: open, no pull request, and an assignee or an `Owner:` line in its body.
pub struct GitHub;

impl Issues for GitHub {
    fn owned_open(&self, issue: &IssueName) -> Result<(), String> {
        let read = north_base::platform::github::json(&["api", &format!("repos/{}/issues/{}", issue.repo, issue.number)]).map_err(|why| format!("{} cannot be read: {}", issue.label(), why.trim()))?;
        judge_issue(issue, &read)
    }
}

pub fn judge_issue(issue: &IssueName, read: &serde_json::Value) -> Result<(), String> {
    if read.get("pull_request").is_some() {
        return Err(format!("{} is a pull request, not a class-fix issue", issue.label()));
    }
    if read["state"].as_str() != Some("open") {
        return Err(format!("{} is not open", issue.label()));
    }
    let assigned = read["assignees"].as_array().is_some_and(|people| !people.is_empty());
    let named = read["body"].as_str().unwrap_or_default().lines().any(|line| line.trim_start().to_ascii_lowercase().starts_with("owner:"));
    if assigned || named { Ok(()) } else { Err(format!("{} has no owner: assign it or give it an `Owner:` line", issue.label())) }
}

/// One admission of unblocks: where they are kept, how an issue a repeat names is judged, and the time they are admitted at.
pub struct Admission<'a> {
    pub ledger: &'a Ledger,
    pub issues: &'a dyn Issues,
    pub now: u64,
}

impl Admission<'_> {
    pub fn admit(&self, kind: Kind, class: &str, by: &str, issue: Option<&str>) -> Result<(), String> {
        self.ledger.admit(kind, class, by, issue, self.now, self.issues)
    }
}

pub struct Ledger(pub PathBuf);

impl Ledger {
    pub fn here() -> Self {
        Self(state().join(FILE))
    }

    pub fn rows(&self) -> Result<Vec<Row>, String> {
        let text = match std::fs::read_to_string(&self.0) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("{} cannot be read: {error}", self.0.display())),
        };
        Ok(text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect())
    }

    /// Admits the unblock of `kind` and `class` by `by` at `now` and records it: refused when one of the same kind and
    /// class stands within the day before and `issue` names no open issue with an owner, which `issues` judges.
    pub fn admit(&self, kind: Kind, class: &str, by: &str, issue: Option<&str>, now: u64, issues: &dyn Issues) -> Result<(), String> {
        let named = issue.map(IssueName::parse).transpose()?;
        if let Some(named) = &named {
            issues.owned_open(named).map_err(|why| format!("--issue {}: {why}", named.label()))?;
        }
        let earlier = self.rows()?.into_iter().rfind(|row| row.kind == kind && row.class == class && now.saturating_sub(row.at) < WINDOW_SECONDS && row.at <= now);
        if let (Some(earlier), None) = (&earlier, &named) {
            let ago = now.saturating_sub(earlier.at) / 60;
            return Err(format!(
                "this is the second {} of the class `{class}` within 24 h ({} did the last one {ago} min ago), so its cause needs a fix, not another hand unblock: file or find the open issue that fixes this class, give it an owner and run this again with `--issue REPO#N`",
                kind.label(),
                earlier.by
            ));
        }
        self.append(&Row { at: now, kind, class: class.to_owned(), by: by.to_owned(), issue: named.map(|named| named.label()) })
    }

    fn append(&self, row: &Row) -> Result<(), String> {
        append_line(&self.0, &serde_json::to_string(row).map_err(|error| error.to_string())?)
    }
}

/// The class of a refusal's words: digits and the given names fold away, so the same cause on another session or count is one class.
pub fn refusal_class(why: &str, names: &[&str]) -> String {
    let mut text = why.lines().next().unwrap_or_default().to_owned();
    for name in names.iter().filter(|name| !name.is_empty()) {
        text = text.replace(name, "NAME");
    }
    let mut class = String::with_capacity(text.len());
    let mut digits = false;
    for character in text.chars() {
        if character.is_ascii_digit() {
            if !digits {
                class.push('N');
            }
            digits = true;
        } else {
            class.push(character);
            digits = false;
        }
    }
    class.trim().to_owned()
}

pub fn state() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from)
}

fn append_line(path: &std::path::Path, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{} cannot be created: {error}", parent.display()))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("{} cannot be opened: {error}", path.display()))?;
    file.write_all(format!("{line}\n").as_bytes()).map_err(|error| format!("{} cannot be written: {error}", path.display()))
}

#[derive(Debug, Serialize, Deserialize)]
struct Refused {
    at: u64,
    key: String,
    class: Option<String>,
}

/// The coordinator's launches refused for a task, so its next launch of that task within a day counts as a relaunch after that refusal.
pub struct Refusals(pub PathBuf);

impl Refusals {
    pub fn here() -> Self {
        Self(state().join(REFUSALS))
    }

    /// The class of the refusal the last launch of `key` met within the day before `now`, unless a relaunch answered it since.
    pub fn pending(&self, key: &str, now: u64) -> Result<Option<String>, String> {
        let text = match std::fs::read_to_string(&self.0) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{} cannot be read: {error}", self.0.display())),
        };
        let last = text.lines().rev().filter_map(|line| serde_json::from_str::<Refused>(line).ok()).find(|row| row.key == key);
        Ok(last.filter(|row| now.saturating_sub(row.at) < WINDOW_SECONDS).and_then(|row| row.class))
    }

    pub fn refused(&self, key: &str, class: &str, now: u64) -> Result<(), String> {
        self.write(&Refused { at: now, key: key.to_owned(), class: Some(class.to_owned()) })
    }

    pub fn relaunched(&self, key: &str, now: u64) -> Result<(), String> {
        self.write(&Refused { at: now, key: key.to_owned(), class: None })
    }

    fn write(&self, row: &Refused) -> Result<(), String> {
        append_line(&self.0, &serde_json::to_string(row).map_err(|error| error.to_string())?)
    }
}
