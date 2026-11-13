//! `north dash --bars`: one table of the bars that say the harness is ready for production
//! work, read per repository from the same records as the weekly scoreboard. The thresholds are the
//! `ready` class of north's concerns.toml; days in a row counts the newest consecutive 24 h windows in
//! which every bar passed (docs/rationale/scoreboard.md).
use std::fs;

use crate::cli::scoreboard::{Sources, Window, state_root};

use self::bar::{Bar, DAYS, DAY_S, Goals, REPOS, Reading, Standing, counts};
use self::records::Records;

mod bar;
mod idle;
mod main_green;
mod plain;
pub mod propagation;
mod records;
mod runtime;
mod table;

const NEEDED_DAYS: usize = 3;

/// The `minutes` ending at `now`, bounded as days in a row bounds its newest day.
fn last(now: i64, minutes: i64) -> Window {
    Window { start: now - minutes * 60 + 1, end: now + 1 }
}

/// Records the open priority:now counts for the history the ready table reads.
pub fn record(now: i64) {
    records::record(now);
}

/// One bar's readings for one repository: the last 1 h carries the verdict, 10 min, 3 h, 6 h and 24 h are context.
struct Cells {
    ten_minutes: Option<Reading>,
    hour: (Option<Reading>, Standing),
    three_hours: Option<Reading>,
    six_hours: Option<Reading>,
    day: Option<Reading>,
}

struct Repo {
    name: &'static str,
    bars: Vec<Cells>,
    days_in_a_row: usize,
}

fn assess(reading: impl Fn(Bar, Window) -> Option<Reading>, goals: &Goals, repo: &'static str, now: i64) -> Repo {
    let standing = |bar: Bar, window: Window| {
        let reading = reading(bar, window);
        (reading, Standing::of(goals.of(bar).bound, reading))
    };
    let days_in_a_row = (0..DAYS as i64)
        .map(|day| Window { start: now - (day + 1) * DAY_S + 1, end: now - day * DAY_S + 1 })
        .take_while(|window| Bar::ALL.iter().all(|bar| counts(standing(*bar, *window).1, *bar)))
        .count();
    let bars = Bar::ALL.iter().map(|&bar| Cells { ten_minutes: reading(bar, last(now, 10)), hour: standing(bar, last(now, 60)), three_hours: reading(bar, last(now, 180)), six_hours: reading(bar, last(now, 360)), day: reading(bar, last(now, 1_440)) }).collect();
    Repo { name: repo, bars, days_in_a_row }
}

/// How the ready tables print: `explain` adds each cell's sample and the sources, `color` marks the verdict on a terminal.
#[derive(Clone, Copy, Default)]
pub struct View {
    pub all: bool,
    pub explain: bool,
    pub color: bool,
}

/// The repositories whose projects are not paused in orchestration.toml, or every one with `--all`.
fn shown(all: bool) -> Vec<&'static str> {
    let paused = if all { Vec::new() } else { crate::platform::orchestration::read().map(|cfg| crate::platform::orchestration::set_aside_repos(&cfg)).unwrap_or_default() };
    REPOS.into_iter().filter(|repo| !paused.iter().any(|name| name == repo)).collect()
}

pub fn run(now: i64, view: View) -> Result<String, String> {
    let goals = Goals::read()?;
    let records = Records::read(&Sources::live(), now)?;
    let repos: Vec<Repo> = shown(view.all).into_iter().map(|repo| assess(|bar, window| records.reading(bar, repo, window), &goals, repo, now)).collect();
    Ok(table::render(&repos, &goals, now, view))
}

const TOLD: &str = "north/scoreboard/ready-told";

/// What the proxy hears first each day: the ready verdict of every repository as one line, so Tom reads whether the harness is ready without asking.
pub fn report(now: i64) -> Vec<String> {
    let path = state_root().join(TOLD);
    let last = fs::read_to_string(&path).ok().and_then(|text| text.trim().parse::<i64>().ok()).unwrap_or(0);
    if (0..DAY_S).contains(&(now - last)) {
        return Vec::new();
    }
    let line = match (Goals::read(), Records::read(&Sources::live(), now)) {
        (Ok(goals), Ok(records)) => {
            let heads: Vec<String> = shown(false).into_iter().map(|repo| table::headline(&assess(|bar, window| records.reading(bar, repo, window), &goals, repo, now))).collect();
            format!("ready bar: {} (north dash --bars prints the table)", heads.join("; "))
        }
        (Err(why), _) | (_, Err(why)) => format!("ready bar not read: {why}"),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, now.to_string());
    vec![line]
}

/// Where a target stands: missed over the last 1 h, met now but missed over the window, or met on both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Missed,
    Slipping,
    Met,
}

/// One readiness bar of one repository as `north dash` shows it: its target, the verdict on the last 1 h, and its
/// reading over the last 1 h and over the dash window.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Line {
    pub repo: &'static str,
    pub bar: &'static str,
    #[serde(skip)]
    pub plain: &'static str,
    pub target: String,
    pub verdict: Verdict,
    pub hour: Option<f64>,
    pub hour_n: Option<usize>,
    pub window: Option<f64>,
    pub window_n: Option<usize>,
    #[serde(skip)]
    pub five_text: String,
    #[serde(skip)]
    pub fifteen_text: String,
    #[serde(skip)]
    pub hour_text: String,
    #[serde(skip)]
    pub window_text: String,
}

/// Every bar of every repository not paused, with readings no older than `span` seconds before `now`.
pub fn lines(now: i64, span: i64) -> Result<Vec<Line>, String> {
    let goals = Goals::read()?;
    let records = Records::since(&Sources::live(), now, now - span.max(3_600))?;
    let mut found = Vec::new();
    for repo in shown(false) {
        for bar in Bar::ALL {
            let goal = goals.of(bar);
            let (hour, window) = (records.reading(bar, repo, last(now, 60)), records.reading(bar, repo, last(now, span / 60)));
            let (five, fifteen) = (records.reading(bar, repo, last(now, 5)), records.reading(bar, repo, last(now, 15)));
            found.push(Line {
                repo,
                bar: bar.label(),
                plain: bar.plain(),
                target: table::threshold(goal),
                verdict: match (counts(Standing::of(goal.bound, hour), bar), counts(Standing::of(goal.bound, window), bar)) {
                    (false, _) => Verdict::Missed,
                    (true, false) => Verdict::Slipping,
                    (true, true) => Verdict::Met,
                },
                hour: hour.map(|reading| reading.value),
                hour_n: hour.map(|reading| reading.sample),
                window: window.map(|reading| reading.value),
                window_n: window.map(|reading| reading.sample),
                five_text: table::cell(goal, five, true),
                fifteen_text: table::cell(goal, fifteen, true),
                hour_text: table::cell(goal, hour, true),
                window_text: table::cell(goal, window, true),
            });
        }
    }
    Ok(found)
}
