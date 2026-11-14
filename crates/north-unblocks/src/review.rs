//! The coordinator's hourly class review (north#1577): the harness-loss classes of the last hour with the issue that owns
//! each, which the fleet tick wakes the coordinator with once an hour and the coordinator's stop hook reads, refusing to end
//! its turn while a class seen twice in the hour has no owner issue. An owner is the issue `harness_loss_owners` names for the
//! class, or the last one `north work own CLASS --issue REPO#N` recorded, and stops counting once the GitHub mirror records it closed.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use north_base::platform::attribution::{Cause, Harness};
use north_base::platform::harness::{DEFAULTS, document};
use north_threads::threads::attribution::{Reading, losses};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::{IssueName, Issues};

pub const HOUR_SECONDS: i64 = 60 * 60;
pub const TOP: usize = 5;
const OWNERS: &str = "north/class-owners.jsonl";
const OWNERS_DEFAULT: &str = "harness_loss_owners";

/// One harness class over the hour: its finished worker runs, their dollars and the issue that owns it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassLoss {
    pub class: String,
    pub runs: usize,
    pub usd: f64,
    pub owner: Option<String>,
}

impl ClassLoss {
    pub fn unowned_repeat(&self) -> bool {
        self.runs >= 2 && self.owner.is_none()
    }

    pub fn line(&self) -> String {
        format!("{} {} runs ${:.2}, owner {}", self.class, self.runs, self.usd, self.owner.as_deref().unwrap_or("none"))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Owned {
    at: u64,
    class: String,
    issue: String,
}

/// The issue that owns each harness class: the harness default's, then the last one recorded here.
pub struct Owners {
    pub listed: BTreeMap<String, String>,
    pub recorded: PathBuf,
}

impl Owners {
    pub fn here() -> Self {
        let listed = document(DEFAULTS)[OWNERS_DEFAULT]["value"].as_array().into_iter().flatten().filter_map(|pair| pair.as_str()?.split_once('=')).map(|(class, owner)| (class.trim().to_owned(), owner.trim().to_owned())).collect();
        Self { listed, recorded: crate::state().join(OWNERS) }
    }

    fn recorded(&self) -> Result<BTreeMap<String, String>, String> {
        let text = match std::fs::read_to_string(&self.recorded) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(error) => return Err(format!("{} cannot be read: {error}", self.recorded.display())),
        };
        Ok(text.lines().filter_map(|line| serde_json::from_str::<Owned>(line).ok()).map(|owned| (owned.class, owned.issue)).collect())
    }

    /// The open issue that owns `class`, judging closed issues by `closed`.
    pub fn owner(&self, class: &str, closed: &dyn Fn(&IssueName) -> bool) -> Result<Option<String>, String> {
        let recorded = self.recorded()?;
        let candidates = [recorded.get(class), self.listed.get(class)];
        Ok(candidates.into_iter().flatten().find(|issue| IssueName::parse(issue).is_ok_and(|named| !closed(&named))).cloned())
    }

    /// Records `issue` as the owner of the harness class `class` once `issues` finds it open with an owner of its own.
    pub fn record(&self, class: &str, issue: &str, now: u64, issues: &dyn Issues) -> Result<String, String> {
        if !matches!(Cause::parse(class), Some(Cause::Harness(_))) {
            let known: Vec<&str> = Harness::ALL.iter().map(|harness| harness.label()).collect();
            return Err(format!("{class} is no harness class; the classes are {}", known.join(", ")));
        }
        let named = IssueName::parse(issue)?;
        issues.owned_open(&named)?;
        let line = serde_json::to_string(&Owned { at: now, class: class.to_owned(), issue: named.label() }).map_err(|error| error.to_string())?;
        crate::append_line(&self.recorded, &line)?;
        Ok(format!("{} owns the harness class {class}", named.label()))
    }
}

/// Whether the GitHub mirror records `issue` closed; an issue it does not hold counts as open.
pub fn mirror_closed(issue: &IssueName) -> bool {
    use north_github_mirror::read::{Seen, recorded_issue_state};
    let Some(repo) = issue.repo.strip_prefix("tompassarelli/") else { return false };
    matches!(recorded_issue_state(&north_github_mirror::store::Mirror::local(), repo, issue.number), Some(Seen::Closed | Seen::NotPlanned))
}

/// The worker runs ledger this host reads: `THREADS_DB`, else the snapshot a host without the ledger keeps, else the ledger.
pub fn threads_db() -> PathBuf {
    if let Some(path) = std::env::var_os("THREADS_DB").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    crate::state().join("threads").join(if home.join(".config/threads/remote").exists() { "snapshot.db" } else { "threads.db" })
}

/// The harness losses since `since` in the runs ledger at `db`.
pub fn since(db: &Path, since: i64) -> Result<Reading, String> {
    let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("{}: {why}", db.display()))?;
    losses(&connection, since).map_err(|why| format!("{}: losses: {why}", db.display()))
}

/// Each class of `reading`, costliest first, with the issue that owns it.
pub fn classes(reading: &Reading, owners: &Owners, closed: &dyn Fn(&IssueName) -> bool) -> Result<Vec<ClassLoss>, String> {
    reading.losses.iter().map(|loss| Ok(ClassLoss { class: loss.harness.label().to_owned(), runs: loss.runs, usd: loss.usd, owner: owners.owner(loss.harness.label(), closed)? })).collect()
}

/// Why the coordinator's turn may not end: the classes seen twice in the hour that no issue owns.
pub fn refusal(classes: &[ClassLoss]) -> Option<String> {
    let unowned: Vec<String> = classes.iter().filter(|class| class.unowned_repeat()).map(ClassLoss::line).collect();
    (!unowned.is_empty()).then(|| {
        format!(
            "BLOCKED (class-review): harness-loss classes seen twice in the last hour have no owner issue: {}. File or find the open issue that fixes each class, give it an owner, and record it with `north work own CLASS --issue REPO#N` (or list it in harness_loss_owners); `north work --since 1h` shows the classes",
            unowned.join("; ")
        )
    })
}

/// The hourly review the fleet tick wakes the coordinator with: the top classes of the hour, or nothing in an hour with none.
pub fn message(classes: &[ClassLoss]) -> Option<String> {
    if classes.is_empty() {
        return None;
    }
    let top: Vec<String> = classes.iter().take(TOP).map(ClassLoss::line).collect();
    let mut text = format!("Hourly class review (`north work --since 1h`): the top harness-loss classes of the last hour: {}.", top.join("; "));
    if classes.iter().any(ClassLoss::unowned_repeat) {
        text.push_str(" A class seen twice with no owner issue keeps your turn from ending until `north work own CLASS --issue REPO#N` names the open, owned issue that fixes it.");
    }
    Some(text)
}
