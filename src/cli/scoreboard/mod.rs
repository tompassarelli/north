//! The scoreboard behind `north dash`: the bars that say the harness is ready for production work (ready.rs); `--weekly` prints the harness's weekly numbers for the last 7 days and the 7 days before them,
//! each read from a recorded source, so one week can be set against the last and against published
//! figures (docs/rationale/scoreboard.md).
use std::fmt::Write;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;
use crate::land::trace::profile::percentile;
use crate::ledger::outcome::{self, read::Places};

mod audit;
mod cause;
mod chain;
mod graded;
mod landing;
mod lanes;
mod losses;
mod product;
pub(super) mod ready;
mod tom;
mod weekly;
mod work;
pub(super) mod workers;

const USAGE: &str = "usage: north dash (--bars [--all] [--explain] | --weekly | --record | --rework [--since 7d] | audit [--tell] | losses [--since 24h] [--until DATE] [--repo north])";
const WEEK_S: i64 = 7 * 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: i64,
    pub end: i64,
}

impl Window {
    pub fn holds(self, at: i64) -> bool {
        self.start <= at && at < self.end
    }

    fn weeks(now: i64) -> [Self; 2] {
        [Self { start: now - WEEK_S, end: now }, Self { start: now - 2 * WEEK_S, end: now - WEEK_S }]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Better {
    Lower,
    Higher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spent {
    Dollars,
    Minutes,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Figure {
    Rate { count: usize, hours: f64 },
    Spread { seconds: Vec<f64>, open: usize, open_label: &'static str },
    Share { hits: usize, of: usize, pending: usize, pending_label: &'static str },
    Per { spent: f64, kind: Spent, verified: usize },
    Product { product: product::Product, counted: product::Counted },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    pub name: String,
    pub better: Better,
    pub source: &'static str,
    pub weeks: [Figure; 2],
}

/// One week's report: the measures, and the model spend the money line sets beside the infra total.
pub(super) struct Weekly {
    pub measures: Vec<Measure>,
    /// API-equivalent model spend, this week then the week before.
    pub spend: [f64; 2],
}

pub(super) fn duration(seconds: f64) -> String {
    match seconds {
        value if value < 120.0 => format!("{value:.0} s"),
        value if value < 7_200.0 => format!("{:.0} min", value / 60.0),
        value => format!("{:.1} h", value / 3_600.0),
    }
}

impl Spent {
    fn amount(self, value: f64) -> String {
        match self {
            Self::Dollars => format!("${value:.2}"),
            Self::Minutes => format!("{value:.0} min"),
        }
    }
}

impl Figure {
    /// The week's headline number and how many observations stand behind it.
    pub fn headline(&self) -> Option<(f64, usize)> {
        match self {
            Self::Rate { hours, count } => (*hours > 0.0).then(|| (*count as f64 / hours, *hours as usize)),
            Self::Spread { seconds, .. } => (!seconds.is_empty()).then(|| (percentile(seconds, 0.5), seconds.len())),
            Self::Share { hits, of, .. } => (*of > 0).then_some((*hits as f64 / *of as f64, *of)),
            Self::Per { spent, verified, .. } => (*verified > 0).then_some((*spent / *verified as f64, *verified)),
            Self::Product { product, .. } => product.share().map(|share| (share, product.runs)),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Rate { hours, .. } if *hours <= 0.0 => "no record".into(),
            Self::Rate { count, hours } => format!("{:.2} ({count} in {hours:.0} active h)", *count as f64 / hours),
            Self::Spread { seconds, open, .. } if seconds.is_empty() && *open == 0 => "no record".into(),
            Self::Spread { seconds, open, open_label } => {
                let spread = if seconds.is_empty() { "none landed".into() } else { format!("median {}, p90 {}", duration(percentile(seconds, 0.5)), duration(percentile(seconds, 0.9))) };
                let open = if *open > 0 { format!(", {open} {open_label}") } else { String::new() };
                format!("{spread} (n={}{open})", seconds.len())
            }
            Self::Per { spent, kind, verified } => match (spent, verified) {
                (spent, 0) if *spent <= 0.0 => "no record".into(),
                (spent, 0) => format!("none verified ({})", kind.amount(*spent)),
                (spent, verified) => format!("{} per verified landing (n={verified}, {} in all)", kind.amount(*spent / *verified as f64), kind.amount(*spent)),
            },
            Self::Product { product, counted } => product.text(*counted),
            Self::Share { of: 0, pending: 0, .. } => "no record".into(),
            Self::Share { hits, of, pending, pending_label } => {
                let share = if *of > 0 { format!("{:.0}%", *hits as f64 / *of as f64 * 100.0) } else { "n/a".into() };
                let pending = if *pending > 0 { format!(", {pending} {pending_label}") } else { String::new() };
                format!("{share} ({hits}/{of}{pending})")
            }
        }
    }
}

pub(super) fn render(report: &Weekly, infra: Result<String, String>, now: i64) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "north dash --weekly: last 7 days | the 7 days before, ending {} today", Timestamp::from_unix_seconds(now).local_twelve_hour());
    let infra = infra.map_or_else(|why| format!("infra unread: {why}"), |total| format!("infra {total} a month"));
    let _ = writeln!(out, "money: model spend ${:.2} this week, ${:.2} the week before | {infra}", report.spend[0], report.spend[1]);
    for (index, measure) in report.measures.iter().enumerate() {
        let _ = writeln!(out, "- {}: {} | {}", measure.name, measure.weeks[0].text(), measure.weeks[1].text());
        if report.measures.get(index + 1).is_none_or(|next| next.source != measure.source) {
            let _ = writeln!(out, "  from {}", measure.source);
        }
    }
    out
}

struct Sources {
    threads: PathBuf,
    places: Places,
    home: PathBuf,
    records: PathBuf,
}

pub(super) fn state_root() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state"), PathBuf::from)
}

impl Sources {
    fn live() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let state = state_root();
        let threads = crate::ledger::threads_database();
        Self { places: Places::live(&threads, &state), threads, home, records: crate::platform::records_mirror::read("north dash").home().to_owned() }
    }
}

pub(super) fn open(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("{}: {why}", path.display()))
}

fn measure(sources: &Sources, now: i64) -> Result<Weekly, String> {
    let weeks = Window::weeks(now);
    let threads = open(&sources.threads)?;
    let failed = |why: rusqlite::Error| format!("{}: {why}", sources.threads.display());
    let tom = tom::Tom::read(&sources.places, &sources.home, weeks[1].start, now)?;
    let firsts = landing::first_landings(&threads).map_err(failed)?;
    let mut measures = vec![tom.corrections(weeks), tom.asks(weeks, &firsts)];
    measures.push(landing::queue_to_main(&threads, weeks).map_err(failed)?);
    measures.push(landing::verification(&sources.records, weeks));
    measures.push(landing::rework(&threads, &sources.places.rework, weeks, now)?);
    measures.extend(workers::first_try(&threads, weeks).map_err(failed)?);
    measures.push(tom.recommendations(weeks));
    let outcome = outcome::read::read_weeks(&sources.places, tom.typed(), now)?;
    let spend = std::array::from_fn(|week| outcome.get(week).map_or(0.0, |week| week.total.usd));
    measures.extend(work::per_verified(&outcome));
    measures.push(graded::graded(crate::ledger::closed_report_and_previous("7d").ok().as_ref()));
    measures.extend(product::product_share(&threads, weeks).map_err(|why| format!("{}: {why}", sources.threads.display()))?);
    Ok(Weekly { measures, spend })
}

fn harness_lanes(sources: &Sources, now: i64) -> String {
    let window = Window::weeks(now)[0];
    match open(&sources.threads).and_then(|db| lanes::lanes(&db, &sources.places.rework, window)) {
        Ok(found) => lanes::render(&found),
        Err(why) => format!("harness lanes: unread: {why}\n"),
    }
}

pub fn tick(now: i64) -> Vec<String> {
    ready::record(now);
    audit::tick(now);
    let mut told = ready::report(now);
    told.extend(weekly::tick(now));
    told
}

fn rework(request: &cause::Request) -> u8 {
    let now = Timestamp::now().unix_seconds();
    match cause::run(&Sources::live(), request.window(now), request.label(), now) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(why) => {
            eprintln!("north dash: {why}");
            1
        }
    }
}

fn ready_table(flags: &[String]) -> u8 {
    let mut view = ready::View { color: std::io::stdout().is_terminal(), ..ready::View::default() };
    for flag in flags {
        match flag.as_str() {
            "--all" => view.all = true,
            "--explain" => view.explain = true,
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    match ready::run(Timestamp::now().unix_seconds(), view) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(why) => {
            eprintln!("north dash: {why}");
            1
        }
    }
}

/// `infra_total` reads the live infra total the money line prints, so the caller owns the read model and this module imports no view.
pub fn run(arguments: &[String], infra_total: fn() -> Result<String, String>) -> u8 {
    let recording = match arguments {
        [word, rest @ ..] if word == "audit" => return audit::run(rest),
        [word, rest @ ..] if word == "losses" => return losses::run(rest),
        [flag] if flag == "--weekly" => false,
        [flag] if flag == "--record" => true,
        [] => return ready_table(arguments),
        [flag, ..] if flag == "--all" || flag == "--explain" => return ready_table(arguments),
        _ => match cause::Request::parse(arguments) {
            Some(request) => return rework(&request),
            None => {
                eprintln!("{USAGE}");
                return 64;
            }
        },
    };
    let now = Timestamp::now().unix_seconds();
    let sources = Sources::live();
    let measured = if recording { weekly::record(&sources, now).map(|(record, report)| (report, Some(record))) } else { measure(&sources, now).map(|report| (report, None)) };
    match measured {
        Ok((report, record)) => {
            print!("{}", render(&report, infra_total(), now));
            if record.is_none() {
                print!("{}", harness_lanes(&sources, now));
            }
            if let Some(record) = record {
                println!("{}", record.score());
            }
            0
        }
        Err(why) => {
            eprintln!("north dash: {why}");
            1
        }
    }
}
