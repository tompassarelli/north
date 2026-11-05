//! `north tune`: the daily learning tick. It reads the ledger's closed boxes of the last week and
//! the week before, prints the dashboard line, writes the routing and effort cells whose arms decided and the
//! ETA and budget cells to the cells file, appends each changed cell's evidence to the provenance
//! file, keeps one rolling north issue for the cells whose cost per closed box rose more than 25%
//! week on week, opened on the second risen tick in a row and closed on the first calm one, and prints the open north issue
//! count. It keeps each staffed cell's cost per closed box once a day in the costs file and prints
//! today's beside the last earlier day's. It reads each landed plan issue's dividend from the same
//! week, keeps the reading once a day in the readings file, and tells the proxy once when one turns
//! DECAYED. It prints each tier and category's estimates against what the week's runs came to and
//! files one north issue for each whose median ratio drifted out of the calibrated band, under the
//! same rule as a rise. It prints the share of the last day's finished runs that recorded each
//! essential field and fails when one is under 98%. A failed read of the arms' evidence keeps the
//! prior routing cells and fails the tick.
use crate::foundation::clock::Timestamp;
use crate::ledger::{self, ClosedReport, EstimatedRun};
use crate::dispatch::{Evidence, LEDGER_WINDOW, arm_evidence, evidenced};
use crate::platform::github;
use crate::platform::rolling_issue::{self, Breaches, alarm};
use cells::{Cells, Change};
use costs::Cost;
use dividends::{Dividend, Reading};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

mod cells;
mod costs;
mod coverage;
mod dividends;
mod estimates;
mod rises;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north tune [--dry-run]";
const REPO: &str = "tompassarelli/north";

/// The north issue tracker a tick reads and files to.
pub trait Tracker {
    /// The titles of the open north issues.
    fn open(&self) -> Result<Vec<String>, String>;
    /// The titles of the north issues closed in the last week, which still hold their cell.
    fn recently_closed(&self) -> Result<Vec<String>, String>;
    /// Files an issue and returns its URL.
    fn file(&mut self, title: &str, body: &str) -> Result<String, String>;
    /// Sends the proxy session a fleet message.
    fn tell_proxy(&mut self, text: &str) -> Result<(), String>;
    fn rolling(&mut self) -> &mut dyn rolling_issue::Tracker;
}

/// The ledger as one tick reads it; `notes` are the reads that failed and `runs` the last day's
/// finished runs.
pub struct Ledger {
    pub week: ClosedReport,
    pub before: ClosedReport,
    pub evidence: BTreeMap<String, Evidence>,
    pub notes: Vec<String>,
    pub estimates: Result<Vec<EstimatedRun>, String>,
    pub runs: Result<Vec<Value>, String>,
}

/// What earlier ticks left: the cells they wrote, the costs and dividend readings they kept, and the
/// risen ticks in a row the rolling issue has seen.
#[derive(Default)]
pub struct Prior {
    pub cells: Cells,
    pub costs: Vec<Cost>,
    pub readings: Vec<Reading>,
    pub breaches: Breaches,
}

#[derive(Debug, PartialEq)]
pub struct Tick {
    pub lines: Vec<String>,
    pub cells: Cells,
    pub records: Vec<Value>,
    pub costs: Vec<Value>,
    pub readings: Vec<Value>,
    pub breaches: Breaches,
    pub failed: bool,
}

pub fn tick(ledger: &Ledger, prior: &Prior, dividends: &[Dividend], tracker: &mut dyn Tracker, at: &str, dry_run: bool) -> Tick {
    let mut lines = vec![ledger.week.dashboard(LEDGER_WINDOW)];
    lines.extend(ledger.notes.iter().map(|note| format!("unread: {note}; prior routing cells are kept")));
    let mut decided = cells::decide(&ledger.week, &ledger.evidence);
    if !ledger.notes.is_empty() {
        cells::carry_routes(&prior.cells, &mut decided);
    }
    let changes = cells::changes(&prior.cells, &decided);
    lines.extend(changes.iter().map(Change::line));
    if changes.is_empty() {
        lines.push(format!("no cell changed ({} cells)", decided.len()));
    }
    let today = costs::today(&ledger.week, &prior.costs, at);
    lines.extend(today.lines);
    let checked = dividends::check(&ledger.week, dividends, &prior.readings, at);
    lines.extend(checked.lines);
    let (readings, told_failed) = tell_decays(tracker, checked.rows, checked.alerts, dry_run, &mut lines);
    let calibrations = match &ledger.estimates {
        Ok(runs) => estimates::calibrations(runs),
        Err(error) => {
            lines.push(format!("estimates: unread ({error})"));
            Vec::new()
        }
    };
    lines.extend(calibrations.iter().map(estimates::Calibration::line));
    let (covered, uncovered) = coverage::check(&ledger.runs);
    lines.extend(covered);
    let mut breaches = prior.breaches.clone();
    let risen = keep_rises(&rises::rises(&ledger.week, &ledger.before), tracker, &mut breaches, at, dry_run, &mut lines);
    let findings = calibrations.iter().filter(|calibration| calibration.drifted()).map(|calibration| Finding { line: calibration.drift_line(), title: calibration.title(), body: calibration.body(at) }).collect();
    let unfiled = file_findings(findings, tracker, dry_run, &mut lines);
    let open = tracker.open().map(|open| open.len());
    lines.push(match &open {
        Ok(open) => format!("open north issues: {open}"),
        Err(error) => format!("open north issues: unread ({error})"),
    });
    let failed = !ledger.notes.is_empty() || ledger.estimates.is_err() || risen.is_err() || unfiled || open.is_err() || told_failed || uncovered;
    Tick { lines, cells: cells::cells(&decided), records: changes.iter().map(|change| change.record(at)).collect(), costs: today.records, readings, breaches, failed }
}

fn keep_rises(found: &[rises::Rise], tracker: &mut dyn Tracker, breaches: &mut Breaches, at: &str, dry_run: bool, lines: &mut Vec<String>) -> Result<(), String> {
    lines.extend(found.iter().map(rises::Rise::line));
    let body = rises::issue_body(found);
    if dry_run {
        if body.is_some() {
            lines.push(format!("  would file: {}", rises::TITLE));
        }
        return Ok(());
    }
    let closing = format!("`north tune` on {at}: no staffed cell's cost per closed box sits more than 25% above the week before.");
    alarm(tracker.rolling(), breaches, rises::TITLE, body.as_deref(), &closing).inspect_err(|error| lines.push(format!("  not settled: {}: {error}", rises::TITLE)))
}

/// Tells the proxy of each dividend that decayed and returns the rows to keep: a dividend whose
/// message did not go keeps no row, so the next tick tells again. A dry run tells no one.
fn tell_decays(tracker: &mut dyn Tracker, rows: Vec<(String, Value)>, alerts: Vec<(String, String)>, dry_run: bool, lines: &mut Vec<String>) -> (Vec<Value>, bool) {
    let mut untold = Vec::new();
    for (name, text) in alerts {
        if dry_run {
            lines.push(format!("  would tell the proxy: {text}"));
        } else if let Err(error) = tracker.tell_proxy(&text) {
            lines.push(format!("  proxy not told of {name}: {error}"));
            untold.push(name);
        } else {
            lines.push(format!("  told the proxy: {name} decayed"));
        }
    }
    let kept = rows.into_iter().filter(|(name, _)| !untold.contains(name)).map(|(_, row)| row).collect();
    (kept, !untold.is_empty())
}

/// A cell a tick files an issue for: the line it prints, and the issue's title and body.
struct Finding {
    line: String,
    title: String,
    body: String,
}

/// Files each found cell that no open or recently closed issue holds, and says whether a finding
/// went unfiled for a failure rather than a holding issue or a dry run.
fn file_findings(findings: Vec<Finding>, tracker: &mut dyn Tracker, dry_run: bool, lines: &mut Vec<String>) -> bool {
    let held = tracker.open().and_then(|open| Ok([open, tracker.recently_closed()?].concat()));
    let mut failed = false;
    for Finding { line, title, body } in findings {
        lines.push(line);
        lines.push(match &held {
            Err(error) => {
                failed = true;
                format!("  not filed: the issues that may hold it are unread ({error})")
            }
            Ok(held) if held.contains(&title) => format!("  held by an issue filed for it: {title}"),
            Ok(_) if dry_run => format!("  would file: {title}"),
            Ok(_) => match tracker.file(&title, &body) {
                Ok(url) => format!("  filed {url}"),
                Err(error) => {
                    failed = true;
                    format!("  not filed: {error}")
                }
            },
        });
    }
    failed
}

pub fn run(arguments: &[String]) -> u8 {
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
    let evidence = arm_evidence(&evidenced(), &mut notes);
    let estimates = ledger::estimated_runs(LEDGER_WINDOW).map_err(|error| error.user_message());
    let ledger = Ledger { week, before, evidence, notes, estimates, runs: coverage::read() };
    let files = Files::here()?;
    let at = Timestamp::now().iso();
    let prior = Prior { cells: files.cells()?, costs: files.costs()?, readings: files.readings()?, breaches: files.breaches()? };
    let tick = tick(&ledger, &prior, &dividends::definitions()?, &mut Gh { rolling: rolling_issue::Gh::north() }, &at, dry_run);
    tick.lines.iter().for_each(|line| println!("{line}"));
    match crate::loops::here() {
        Ok(checked) => checked.lines.iter().for_each(|line| println!("{line}")),
        Err(error) => println!("loops: unread ({error})"),
    }
    if !dry_run {
        files.write(&tick)?;
    }
    Ok(tick)
}

pub fn directory() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/state/north/tune"))
}

pub fn cells_file() -> Result<PathBuf, String> {
    Ok(directory()?.join("cells.json"))
}

/// The cells file dispatch's decisions are written to and the provenance file each change of them
/// is appended to, in north's state directory.
struct Files {
    dir: PathBuf,
}

impl Files {
    fn here() -> Result<Self, String> {
        Ok(Self { dir: directory()? })
    }

    fn cells(&self) -> Result<Cells, String> {
        let path = self.dir.join("cells.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Cells::new()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    fn costs(&self) -> Result<Vec<Cost>, String> {
        let path = self.dir.join("costs.jsonl");
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(text.lines().filter_map(|line| Cost::read(&serde_json::from_str(line).ok()?)).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    fn breaches(&self) -> Result<Breaches, String> {
        let path = self.dir.join("breaches.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Breaches::default()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    fn readings(&self) -> Result<Vec<Reading>, String> {
        let path = self.dir.join("dividends.jsonl");
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(text.lines().filter_map(|line| Reading::read(&serde_json::from_str(line).ok()?)).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
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
        let mut costs = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("costs.jsonl")).map_err(failed)?;
        tick.costs.iter().try_for_each(|record| writeln!(costs, "{record}")).map_err(failed)?;
        let mut readings = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("dividends.jsonl")).map_err(failed)?;
        tick.readings.iter().try_for_each(|record| writeln!(readings, "{record}")).map_err(failed)?;
        std::fs::write(self.dir.join("breaches.json"), serde_json::to_string(&tick.breaches).map_err(|error| error.to_string())? + "\n").map_err(failed)?;
        let staged = self.dir.join("cells.json.tmp");
        let text = serde_json::to_string_pretty(&tick.cells).map_err(|error| error.to_string())?;
        let mut file = std::fs::File::create(&staged).map_err(failed)?;
        file.write_all((text + "\n").as_bytes()).and_then(|()| file.sync_all()).map_err(failed)?;
        std::fs::rename(&staged, self.dir.join("cells.json")).map_err(failed)
    }
}

struct Gh {
    rolling: rolling_issue::Gh,
}

fn tell(text: &str) -> Result<(), String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let status = std::process::Command::new(north).args(["fleet", "send", "--from", "service:north-tune", "proxy"]).arg(text).status().map_err(|error| format!("cannot run north fleet send: {error}"))?;
    status.success().then_some(()).ok_or_else(|| format!("north fleet send proxy: {status}"))
}

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
        let closed = |phrase: &str| titles("closed", Some(&format!("closed:>={since} in:title \"{phrase}\"")));
        closed("Estimates drifted outside")
    }

    fn file(&mut self, title: &str, body: &str) -> Result<String, String> {
        let out = github::run(&["issue", "create", "-R", REPO, "--title", title, "--body", body]).map_err(|error| format!("cannot run gh: {error}"))?;
        if !out.status.success() {
            return Err(format!("gh issue create: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn tell_proxy(&mut self, text: &str) -> Result<(), String> {
        tell(text)
    }

    fn rolling(&mut self) -> &mut dyn rolling_issue::Tracker {
        &mut self.rolling
    }
}
