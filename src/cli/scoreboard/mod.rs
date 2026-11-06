//! `north scoreboard`: the harness's weekly numbers for the last 7 days and the 7 days before them,
//! each read from a recorded source, so one week can be set against the last and against published
//! figures (docs/rationale/scoreboard.md).
use std::fmt::Write;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;
use crate::land::trace::profile::percentile;
use crate::ledger::outcome::read::Places;

mod cause;
mod landing;
mod tom;
mod weekly;
mod workers;

const USAGE: &str = "usage: north scoreboard [--record | rework]";
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

#[derive(Clone, Debug, PartialEq)]
pub enum Figure {
    Rate { count: usize, hours: f64 },
    Spread { seconds: Vec<f64>, open: usize, open_label: &'static str },
    Share { hits: usize, of: usize, pending: usize, pending_label: &'static str },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    pub name: String,
    pub better: Better,
    pub source: &'static str,
    pub weeks: [Figure; 2],
}

fn duration(seconds: f64) -> String {
    match seconds {
        value if value < 120.0 => format!("{value:.0} s"),
        value if value < 7_200.0 => format!("{:.0} min", value / 60.0),
        value => format!("{:.1} h", value / 3_600.0),
    }
}

impl Figure {
    /// The week's headline number and how many observations stand behind it.
    pub fn headline(&self) -> Option<(f64, usize)> {
        match self {
            Self::Rate { hours, count } => (*hours > 0.0).then(|| (*count as f64 / hours, *hours as usize)),
            Self::Spread { seconds, .. } => (!seconds.is_empty()).then(|| (percentile(seconds, 0.5), seconds.len())),
            Self::Share { hits, of, .. } => (*of > 0).then(|| (*hits as f64 / *of as f64, *of)),
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
            Self::Share { of: 0, pending: 0, .. } => "no record".into(),
            Self::Share { hits, of, pending, pending_label } => {
                let share = if *of > 0 { format!("{:.0}%", *hits as f64 / *of as f64 * 100.0) } else { "n/a".into() };
                let pending = if *pending > 0 { format!(", {pending} {pending_label}") } else { String::new() };
                format!("{share} ({hits}/{of}{pending})")
            }
        }
    }
}

pub fn render(measures: &[Measure], now: i64) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "north scoreboard: last 7 days | the 7 days before, ending {} today", Timestamp::from_unix_seconds(now).local_twelve_hour());
    for (index, measure) in measures.iter().enumerate() {
        let _ = writeln!(out, "- {}: {} | {}", measure.name, measure.weeks[0].text(), measure.weeks[1].text());
        if measures.get(index + 1).is_none_or(|next| next.source != measure.source) {
            let _ = writeln!(out, "  from {}", measure.source);
        }
    }
    out
}

struct Sources {
    threads: PathBuf,
    places: Places,
    home: PathBuf,
}

fn state_root() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state"), PathBuf::from)
}

impl Sources {
    fn live() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let state = state_root();
        let threads = crate::ledger::threads_database();
        Self { places: Places::live(&threads, &state), threads, home }
    }
}

fn open(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("{}: {why}", path.display()))
}

fn measure(sources: &Sources, now: i64) -> Result<Vec<Measure>, String> {
    let weeks = Window::weeks(now);
    let threads = open(&sources.threads)?;
    let failed = |why: rusqlite::Error| format!("{}: {why}", sources.threads.display());
    let tom = tom::Tom::read(&sources.places, &sources.home, weeks[1].start, now)?;
    let firsts = landing::first_landings(&threads).map_err(failed)?;
    let mut measures = vec![tom.corrections(weeks), tom.asks(weeks, &firsts)];
    measures.push(landing::queue_to_main(&threads, weeks).map_err(failed)?);
    measures.push(landing::verification(weeks));
    measures.push(landing::rework(&threads, &sources.places.rework, weeks, now)?);
    measures.extend(workers::first_try(&threads, weeks).map_err(failed)?);
    measures.push(tom.recommendations(weeks));
    Ok(measures)
}

pub fn tick(now: i64) -> Vec<String> {
    weekly::tick(now)
}

fn rework() -> u8 {
    let now = Timestamp::now().unix_seconds();
    match cause::run(&Sources::live(), Window::weeks(now)) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(why) => {
            eprintln!("north scoreboard: {why}");
            1
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let recording = match arguments {
        [] => false,
        [word] if word == "rework" => return rework(),
        [flag] if flag == "--record" => true,
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    let now = Timestamp::now().unix_seconds();
    let sources = Sources::live();
    let measured = if recording { weekly::record(&sources, now).map(|(record, measures)| (measures, Some(record))) } else { measure(&sources, now).map(|measures| (measures, None)) };
    match measured {
        Ok((measures, record)) => {
            print!("{}", render(&measures, now));
            if let Some(record) = record {
                println!("{}", record.score());
            }
            0
        }
        Err(why) => {
            eprintln!("north scoreboard: {why}");
            1
        }
    }
}
