//! `north tune`: the daily learning tick. It reads the ledger's closed boxes of the last week and
//! the week before, prints the dashboard line, writes the ETA and budget cells to the graph as one
//! transaction of writer `tune`, appends each changed cell's evidence to the provenance
//! file, prints the cells whose cost per closed box rose more than 25% week on week and tells the
//! proxy once whenever that set of cells changes, and prints the open north issue count. It keeps each staffed cell's cost per closed box once a day in the costs file and prints
//! today's beside the last earlier day's. It reads each landed plan issue's dividend from the same
//! week, keeps the reading once a day in the readings file, and tells the proxy once when one turns
//! DECAYED. It prints each tier and category's estimates against what the week's runs came to and
//! prints the cells whose ratio at the quantile their estimator covers drifted out of the calibrated band, where an estimate replayed
//! from their own runs of the 7 days before each run would have sat inside it, telling the proxy once whenever that set changes, and counts beside it the cells outside the band under every estimator. It scores each category's runs against the Budget stored at their launch
//! (share over it, actual/E p50 and p90) and reads the budget cells' multiplier from that p90 once
//! a category holds enough runs carrying one, and moves an ETA or budget cell at most 2x from its prior value per tick. It prints the share of the last day's finished runs
//! that recorded each essential field and fails when one is under 98%. It prints each skill's
//! closed-box rate and cost against matched boxes that did not load it, and flags the installed
//! skills no run loaded and those whose boxes did worse. Before the loop check it runs the skill switch's backtest (`switch`) and tells each
//! decision's reading to the graph; `--backtest` prints that table alone and tells nothing.
use crate::foundation::clock::Timestamp;
use crate::ledger::{self, ClosedReport, rises};
use crate::dispatch::tuned::{WRITER, directory};
use crate::dispatch::LEDGER_WINDOW;
use crate::threads::calibration::predictions::Brier;
use crate::platform::github;
use cells::{Cells, Change};
use north_graph_loops::Learned;
use costs::Cost;
use dividends::{Dividend, Reading};
use estimates::EstimatedRuns;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

mod budgets;
mod cells;
mod costs;
mod coverage;
mod dividends;
mod estimates;
mod skills;
pub mod switch;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north tune [--dry-run | --backtest]";
const REPO: &str = "tompassarelli/north";
const REPLAY_WINDOW: &str = "14d";

/// The north issue tracker a tick reads and the proxy it tells.
pub trait Tracker {
    /// How many north issues are open.
    fn open(&self) -> Result<usize, String>;
    /// Sends the proxy session a fleet message.
    fn tell_proxy(&mut self, text: &str) -> Result<(), String>;
}

/// The ledger as one tick reads it, with the skills agents can load and `runs` the last day's
/// finished runs.
pub struct Ledger {
    pub week: ClosedReport,
    pub before: ClosedReport,
    pub estimates: Result<EstimatedRuns, String>,
    pub briers: Result<Vec<Brier>, String>,
    pub runs: Result<Vec<Value>, String>,
    pub skills: Vec<String>,
}

/// The cells each week-window alarm last told the proxy of, by alarm.
pub type Alarms = BTreeMap<String, BTreeSet<String>>;

/// What earlier ticks left: the cells they wrote, the costs and dividend readings they kept, and the
/// cells each alarm last told the proxy of.
#[derive(Default)]
pub struct Prior {
    pub cells: Cells,
    pub costs: Vec<Cost>,
    pub readings: Vec<Reading>,
    pub alarms: Alarms,
}

#[derive(Debug, PartialEq)]
pub struct Tick {
    pub lines: Vec<String>,
    pub cells: Cells,
    pub records: Vec<Value>,
    pub costs: Vec<Value>,
    pub readings: Vec<Value>,
    pub alarms: Alarms,
    pub failed: bool,
}

pub fn tick(ledger: &Ledger, prior: &Prior, dividends: &[Dividend], tracker: &mut dyn Tracker, at: &str, dry_run: bool) -> Tick {
    let mut lines = vec![ledger.week.dashboard(LEDGER_WINDOW)];
    let scores = ledger.estimates.as_ref().map(|runs| budgets::scores(&runs.week)).unwrap_or_default();
    let mut decided = cells::decide(&ledger.week, &budgets::multipliers(&scores));
    cells::limit_steps(&prior.cells, &mut decided);
    let changes = cells::changes(&prior.cells, &decided);
    lines.extend(changes.iter().map(Change::line));
    if changes.is_empty() {
        lines.push(format!("no cell changed ({} cells)", decided.len()));
    }
    let today = costs::today(&ledger.week, &prior.costs, at);
    lines.extend(today.lines);
    lines.extend(skills::lines(&ledger.week, &ledger.before, &ledger.skills));
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
    if ledger.estimates.is_ok() {
        lines.push(budgets::summary(LEDGER_WINDOW, &scores));
        lines.extend(scores.iter().map(budgets::Score::line));
    }
    match &ledger.briers {
        Ok(briers) if briers.is_empty() => lines.push(format!("tier success: no first run in {LEDGER_WINDOW} carries a stored prediction")),
        Ok(briers) => lines.extend(briers.iter().map(|brier| brier.line(LEDGER_WINDOW))),
        Err(error) => lines.push(format!("tier success: unread ({error})")),
    }
    let (covered, uncovered) = coverage::check(&ledger.runs);
    lines.extend(covered);
    let mut alarms = prior.alarms.clone();
    let found_rises = rises::rises(&ledger.week, &ledger.before);
    lines.extend(found_rises.iter().map(rises::Rise::line));
    let risen = tell_alarm(tracker, &mut alarms, rises::TITLE, found_rises.iter().map(|rise| format!("{} {}", rise.tier, rise.category)).collect(), dry_run, &mut lines);
    lines.extend(calibrations.iter().filter(|calibration| calibration.drifted()).map(estimates::Calibration::drift_line));
    if ledger.estimates.is_ok() {
        lines.extend(estimates::noise_lines(&calibrations));
    }
    let drifted = tell_alarm(tracker, &mut alarms, estimates::TITLE, calibrations.iter().filter(|calibration| calibration.drifted()).map(|calibration| format!("{} {}", calibration.tier, calibration.category)).collect(), dry_run, &mut lines);
    let open = tracker.open();
    lines.push(match &open {
        Ok(open) => format!("open north issues: {open}"),
        Err(error) => format!("open north issues: unread ({error})"),
    });
    let failed = ledger.estimates.is_err() || ledger.briers.is_err() || risen || drifted || open.is_err() || told_failed || uncovered;
    Tick { lines, cells: cells::cells(&decided), records: changes.iter().map(|change| change.record(at)).collect(), costs: today.records, readings, alarms, failed }
}

/// Tells the proxy when the set of cells under an alarm changed since the last tell, and keeps the
/// set told; a tell that did not go keeps the earlier set, so the next tick tells again. A dry run
/// tells no one.
fn tell_alarm(tracker: &mut dyn Tracker, alarms: &mut Alarms, title: &str, cells: BTreeSet<String>, dry_run: bool, lines: &mut Vec<String>) -> bool {
    if alarms.get(title).map_or(cells.is_empty(), |told| *told == cells) {
        return false;
    }
    let text = if cells.is_empty() {
        format!("north tune: {title}: no cell any longer; `north tune --dry-run` shows the week's cells.")
    } else {
        format!("north tune: {title}: {}; `north tune --dry-run` shows them.", cells.iter().cloned().collect::<Vec<_>>().join(", "))
    };
    if dry_run {
        lines.push(format!("  would tell the proxy: {text}"));
        return false;
    }
    if let Err(error) = tracker.tell_proxy(&text) {
        lines.push(format!("  proxy not told of {title}: {error}"));
        return true;
    }
    lines.push(format!("  told the proxy: {title}: {} cells", cells.len()));
    if cells.is_empty() {
        alarms.remove(title);
    } else {
        alarms.insert(title.to_owned(), cells);
    }
    false
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

pub fn run(arguments: &[String]) -> u8 {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        [flag] if flag == "--backtest" => {
            switch::judge(true).iter().for_each(|line| println!("{line}"));
            return 0;
        }
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

pub fn failing(tick: Tick, loops: &Result<crate::loops::Checked, String>) -> Tick {
    let failed = tick.failed || !loops.as_ref().is_ok_and(|checked| checked.failing.is_empty());
    Tick { failed, ..tick }
}

fn tune(dry_run: bool) -> Result<Tick, String> {
    let (week, before) = ledger::closed_report_and_previous(LEDGER_WINDOW).map_err(|error| error.user_message())?;
    let estimates = estimated_runs();
    let root = crate::foundation::activation::activation_paths(&|name: &str| std::env::var(name).ok().filter(|value| !value.is_empty())).0.with_file_name("skills/shared");
    let skills = skills::installed(&root).unwrap_or_else(|error| {
        eprintln!("north tune: installed skills unread, so none is flagged unused: {error}");
        Vec::new()
    });
    let briers = ledger::tier_briers(LEDGER_WINDOW).map_err(|error| error.user_message());
    let ledger = Ledger { week, before, estimates, briers, runs: coverage::read(), skills };
    let files = Files::here()?;
    let at = Timestamp::now().iso();
    let learned = files.learned()?;
    let prior = Prior { cells: learned.cells.clone(), costs: files.costs()?, readings: files.readings()?, alarms: files.alarms()? };
    let tick = tick(&ledger, &prior, &dividends::definitions()?, &mut Gh, &at, dry_run);
    tick.lines.iter().for_each(|line| println!("{line}"));
    switch::judge(dry_run).iter().for_each(|line| println!("{line}"));
    let loops = crate::loops::here(None);
    match &loops {
        Ok(checked) => checked.lines.iter().for_each(|line| println!("{line}")),
        Err(error) => println!("loops: unread ({error})"),
    }
    let tick = failing(tick, &loops);
    if !dry_run {
        files.write(&tick, &learned)?;
    }
    Ok(tick)
}

/// The graph dispatch's cells are told to, and the provenance, costs, readings and alarms files beside
/// them in north's state directory.
struct Files {
    dir: PathBuf,
}

impl Files {
    fn here() -> Result<Self, String> {
        Ok(Self { dir: directory()? })
    }

    fn learned(&self) -> Result<Learned, String> {
        let graph = north_graph_loops::graph_path();
        if !graph.exists() {
            return Ok(Learned::default());
        }
        north_graph_loops::read(&graph, WRITER)
    }

    fn costs(&self) -> Result<Vec<Cost>, String> {
        let path = self.dir.join("costs.jsonl");
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(text.lines().filter_map(|line| Cost::read(&serde_json::from_str(line).ok()?)).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    fn alarms(&self) -> Result<Alarms, String> {
        let path = self.dir.join("alarms.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Alarms::default()),
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

    /// Appends the changes' provenance before the graph moves, so a tick that dies between the two
    /// re-records a change next time rather than losing its evidence.
    fn write(&self, tick: &Tick, prior: &Learned) -> Result<(), String> {
        let failed = |error: std::io::Error| format!("{}: {error}", self.dir.display());
        std::fs::create_dir_all(&self.dir).map_err(failed)?;
        let mut provenance = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("provenance.jsonl")).map_err(failed)?;
        tick.records.iter().try_for_each(|record| writeln!(provenance, "{record}")).map_err(failed)?;
        provenance.sync_all().map_err(failed)?;
        let mut costs = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("costs.jsonl")).map_err(failed)?;
        tick.costs.iter().try_for_each(|record| writeln!(costs, "{record}")).map_err(failed)?;
        let mut readings = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("dividends.jsonl")).map_err(failed)?;
        tick.readings.iter().try_for_each(|record| writeln!(readings, "{record}")).map_err(failed)?;
        std::fs::write(self.dir.join("alarms.json"), serde_json::to_string(&tick.alarms).map_err(|error| error.to_string())? + "\n").map_err(failed)?;
        north_graph_loops::tell(&north_graph_loops::graph_path(), WRITER, prior, &tick.cells, Timestamp::now().unix_seconds())
    }
}

struct Gh;

fn tell(text: &str) -> Result<(), String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let status = std::process::Command::new(north).args(["fleet", "send", "--from", "service:north-tune", "proxy"]).arg(text).status().map_err(|error| format!("cannot run north fleet send: {error}"))?;
    status.success().then_some(()).ok_or_else(|| format!("north fleet send proxy: {status}"))
}

fn open_count() -> Result<usize, String> {
    let found = github::json(&["api", &format!("search/issues?q=repo:{REPO}+is:issue+is:open&per_page=1")])?;
    found["total_count"].as_u64().and_then(|count| usize::try_from(count).ok()).ok_or_else(|| "gh: search answered without a total_count".to_owned())
}

impl Tracker for Gh {
    fn open(&self) -> Result<usize, String> {
        open_count()
    }

    fn tell_proxy(&mut self, text: &str) -> Result<(), String> {
        tell(text)
    }
}

fn estimated_runs() -> Result<EstimatedRuns, String> {
    let since = ledger::window_start(LEDGER_WINDOW).map_err(|error| error.user_message())?;
    let (week, earlier) = ledger::estimated_runs(REPLAY_WINDOW).map_err(|error| error.user_message())?.into_iter().partition(|run| run.ended >= since);
    Ok(EstimatedRuns { week, earlier })
}
