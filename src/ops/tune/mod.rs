//! `north tune`: the daily learning tick. It reads the ledger's closed boxes of the last week and
//! the week before, prints the dashboard line, writes the routing cells whose arms decided and the
//! ETA and budget cells to the cells file, appends each changed cell's evidence to the provenance
//! file, files one north issue for each cell whose cost per closed box rose more than 25% week on
//! week unless one filed for it is open or closed in the last week, and prints the open north issue
//! count. A failed read of the arms' evidence keeps the prior routing cells and fails the tick.
use crate::foundation::clock::Timestamp;
use crate::ledger::{self, ClosedReport};
use crate::ops::dispatch::{ARMED, Evidence, LEDGER_WINDOW, arm_evidence};
use crate::platform::github;
use cells::{Cells, Change};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

mod cells;
mod rises;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north tune [--dry-run]";
const REPO: &str = "tompassarelli/north";

/// The north issue tracker a tick reads and files to.
pub(crate) trait Tracker {
    /// The titles of the open north issues.
    fn open(&self) -> Result<Vec<String>, String>;
    /// The titles of the north issues closed in the last week, which still hold their cell.
    fn recently_closed(&self) -> Result<Vec<String>, String>;
    /// Files an issue and returns its URL.
    fn file(&mut self, title: &str, body: &str) -> Result<String, String>;
}

/// The ledger as one tick reads it; `notes` are the reads that failed.
pub(crate) struct Ledger {
    pub week: ClosedReport,
    pub before: ClosedReport,
    pub evidence: BTreeMap<String, Evidence>,
    pub notes: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Tick {
    pub lines: Vec<String>,
    pub cells: Cells,
    pub records: Vec<Value>,
    pub failed: bool,
}

pub(crate) fn tick(ledger: &Ledger, prior: &Cells, tracker: &mut dyn Tracker, at: &str, dry_run: bool) -> Tick {
    let mut lines = vec![ledger.week.dashboard(LEDGER_WINDOW)];
    lines.extend(ledger.notes.iter().map(|note| format!("unread: {note}; prior routing cells are kept")));
    let mut decided = cells::decide(&ledger.week, &ledger.evidence);
    if !ledger.notes.is_empty() {
        cells::carry_routes(prior, &mut decided);
    }
    let changes = cells::changes(prior, &decided);
    lines.extend(changes.iter().map(Change::line));
    if changes.is_empty() {
        lines.push(format!("no cell changed ({} cells)", decided.len()));
    }
    let filing = file_rises(ledger, tracker, at, dry_run, &mut lines);
    lines.push(match &filing.open {
        Ok(open) => format!("open north issues: {open}"),
        Err(error) => format!("open north issues: unread ({error})"),
    });
    let failed = !ledger.notes.is_empty() || filing.failed || filing.open.is_err();
    Tick { lines, cells: cells::cells(&decided), records: changes.iter().map(|change| change.record(at)).collect(), failed }
}

/// What filing the risen cells came to: the open issue count after it, and whether a rise went
/// unfiled for a failure rather than a holding issue or a dry run.
struct Filing {
    open: Result<usize, String>,
    failed: bool,
}

/// Files each risen cell that no open or recently closed issue holds.
fn file_rises(ledger: &Ledger, tracker: &mut dyn Tracker, at: &str, dry_run: bool, lines: &mut Vec<String>) -> Filing {
    let open = tracker.open();
    let held = open.clone().and_then(|open| Ok([open, tracker.recently_closed()?].concat()));
    let (mut filed, mut failed) = (0, false);
    for rise in rises::rises(&ledger.week, &ledger.before) {
        lines.push(rise.line());
        let title = rise.title();
        lines.push(match &held {
            Err(error) => {
                failed = true;
                format!("  not filed: the issues that may hold it are unread ({error})")
            }
            Ok(held) if held.contains(&title) => format!("  held by an issue filed for it: {title}"),
            Ok(_) if dry_run => format!("  would file: {title}"),
            Ok(_) => match tracker.file(&title, &rise.body(at)) {
                Ok(url) => {
                    filed += 1;
                    format!("  filed {url}")
                }
                Err(error) => {
                    failed = true;
                    format!("  not filed: {error}")
                }
            },
        });
    }
    Filing { open: open.map(|open| open.len() + filed), failed }
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match tune(dry_run) {
        Ok(tick) if tick.failed => 1,
        Ok(_) => 0,
        Err(error) => {
            eprintln!("north tune: {error}");
            1
        }
    }
}

fn tune(dry_run: bool) -> Result<Tick, String> {
    let (week, before) = ledger::closed_report_and_previous(LEDGER_WINDOW).map_err(|error| error.user_message())?;
    let mut notes = Vec::new();
    let evidence = arm_evidence(&ARMED, &mut notes);
    let ledger = Ledger { week, before, evidence, notes };
    let files = Files::here()?;
    let at = Timestamp::now().iso();
    let tick = tick(&ledger, &files.cells()?, &mut Gh, &at, dry_run);
    tick.lines.iter().for_each(|line| println!("{line}"));
    if !dry_run {
        files.write(&tick)?;
    }
    Ok(tick)
}

/// The cells file dispatch's decisions are written to and the provenance file each change of them
/// is appended to, in north's state directory.
struct Files {
    dir: PathBuf,
}

impl Files {
    fn here() -> Result<Self, String> {
        let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or("HOME is not set")?;
        Ok(Self { dir: PathBuf::from(home).join(".local/state/north/tune") })
    }

    fn cells(&self) -> Result<Cells, String> {
        let path = self.dir.join("cells.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Cells::new()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    /// Appends the changes' provenance before the cells file moves, so a tick that dies between the
    /// two re-records a change next time rather than losing its evidence.
    fn write(&self, tick: &Tick) -> Result<(), String> {
        let failed = |error: std::io::Error| format!("{}: {error}", self.dir.display());
        std::fs::create_dir_all(&self.dir).map_err(failed)?;
        let mut provenance = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("provenance.jsonl")).map_err(failed)?;
        tick.records.iter().try_for_each(|record| writeln!(provenance, "{record}")).map_err(failed)?;
        provenance.sync_all().map_err(failed)?;
        let staged = self.dir.join("cells.json.tmp");
        let text = serde_json::to_string_pretty(&tick.cells).map_err(|error| error.to_string())?;
        let mut file = std::fs::File::create(&staged).map_err(failed)?;
        file.write_all((text + "\n").as_bytes()).and_then(|()| file.sync_all()).map_err(failed)?;
        std::fs::rename(&staged, self.dir.join("cells.json")).map_err(failed)
    }
}

struct Gh;

fn titles(state: &str, search: Option<&str>) -> Result<Vec<String>, String> {
    let mut args = vec!["issue", "list", "-R", REPO, "--state", state, "--limit", "1000", "--json", "title"];
    args.extend(search.map(|search| ["--search", search]).into_iter().flatten());
    let issues = github::json(&args)?;
    Ok(issues.as_array().into_iter().flatten().filter_map(|issue| issue["title"].as_str().map(str::to_owned)).collect())
}

impl Tracker for Gh {
    fn open(&self) -> Result<Vec<String>, String> {
        titles("open", None)
    }

    fn recently_closed(&self) -> Result<Vec<String>, String> {
        let since = Timestamp::from_unix_seconds(Timestamp::now().unix_seconds() - 7 * 86_400).iso_date();
        titles("closed", Some(&format!("closed:>={since} in:title \"Cost per closed box rose\"")))
    }

    fn file(&mut self, title: &str, body: &str) -> Result<String, String> {
        let out = github::run(&["issue", "create", "-R", REPO, "--title", title, "--body", body]).map_err(|error| format!("cannot run gh: {error}"))?;
        if !out.status.success() {
            return Err(format!("gh issue create: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }
}
