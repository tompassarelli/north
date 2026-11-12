//! The fleet tick's concern line: the proxy hears of red concerns the tick after they turn red, again when
//! the set of red concerns changes and as a reminder every few hours, and a tick reads the oracles at most every few minutes.
use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use north_base::foundation::clock::Timestamp;
use north_base::platform::host;
use serde::{Deserialize, Serialize};

use super::history::{self, Recorded, State as Judged, Store};
use super::trend::{self, Point};
use super::verdict::Verdict;
use super::{PROJECTS, Row, check, code_root, line};

const CHECK_EVERY: i64 = 600;
const REMIND_EVERY: i64 = 6 * 3600;
const LATEST: &str = "north/concerns/latest.json";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Red {
    pub set: String,
    pub text: String,
}

#[derive(Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct State {
    checked_at: i64,
    red: Red,
    announced: String,
    announced_at: i64,
}

pub fn red(rows: &[Row], problems: &[String]) -> Red {
    let bad: Vec<&Row> = rows.iter().filter(|row| !row.verdict.green()).collect();
    if bad.is_empty() && problems.is_empty() {
        return Red::default();
    }
    let set = bad.iter().map(|row| format!("{}/{}:{}", row.project, row.concern.name, row.verdict.word())).chain(problems.iter().cloned()).collect::<Vec<_>>().join(";");
    let lines: Vec<String> = bad.iter().map(|row| line(row)).chain(problems.iter().map(|problem| format!("not read: {problem}"))).collect();
    Red { set, text: format!("concerns red ({}): {}", lines.len(), lines.join("; ")) }
}

pub fn decide(state: State, now: i64, measure: impl FnOnce() -> Red) -> (Option<String>, State) {
    let fresh = (0..CHECK_EVERY).contains(&(now - state.checked_at));
    let (red, checked_at) = if fresh { (state.red.clone(), state.checked_at) } else { (measure(), now) };
    let reminded = (0..REMIND_EVERY).contains(&(now - state.announced_at));
    let tell = !red.set.is_empty() && (red.set != state.announced || !reminded);
    let (announced, announced_at) = match (tell, red.set.is_empty()) {
        (true, _) => (red.set.clone(), now),
        (false, true) => (String::new(), 0),
        (false, false) => (state.announced, state.announced_at),
    };
    (tell.then(|| red.text.clone()), State { checked_at, red, announced, announced_at })
}

pub fn state_file(relative: &str) -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state"), PathBuf::from).join(relative)
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Measured {
    pub at: i64,
    pub rows: usize,
    pub red: usize,
    pub lines: Vec<String>,
}

impl Measured {
    fn of(rows: &[Row], problems: &[String], at: i64) -> Self {
        let lines = problems.iter().cloned().chain(rows.iter().map(line)).collect();
        Self { at, rows: rows.len(), red: rows.iter().filter(|row| !row.verdict.green()).count(), lines }
    }
}

fn measure() -> (Vec<Row>, Vec<String>) {
    let projects: Vec<String> = PROJECTS.iter().map(|name| (*name).to_owned()).collect();
    check(&code_root(), &projects)
}

pub fn latest(now: i64) -> Measured {
    let kept = std::fs::read_to_string(state_file(LATEST)).ok().and_then(|text| serde_json::from_str::<Measured>(&text).ok());
    kept.filter(|measured| (0..CHECK_EVERY).contains(&(now - measured.at))).unwrap_or_else(|| {
        let (rows, problems) = measure();
        let measured = Measured::of(&rows, &problems, now);
        keep(&measured);
        measured
    })
}

pub fn lines(now: i64, history: &dyn Store) -> Vec<String> {
    announce(&state_file("north/concerns/tick.json"), now, || {
        let (rows, problems) = measure();
        record(&rows, history, now);
        keep(&Measured::of(&rows, &problems, now));
        red(&rows, &problems)
    })
}

fn keep(measured: &Measured) {
    let file = state_file(LATEST);
    if let (Some(parent), Ok(text)) = (file.parent(), serde_json::to_string(measured)) {
        let _ = std::fs::create_dir_all(parent).and_then(|()| std::fs::write(&file, text));
    }
}

pub fn announce(file: &Path, now: i64, measure: impl FnOnce() -> Red) -> Vec<String> {
    let state = std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let (told, state) = decide(state, now, measure);
    if let (Some(parent), Ok(text)) = (file.parent(), serde_json::to_string(&state)) {
        let _ = std::fs::create_dir_all(parent).and_then(|()| std::fs::write(file, text));
    }
    told.into_iter().collect()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct Kept {
    shown: Judged,
    last: Judged,
}

pub fn state_of(row: &Row, recorded: &[Recorded], now: i64) -> (Judged, Option<trend::Judged>) {
    let value = match row.verdict {
        Verdict::Unmeasured(_) => return (Judged::Unmeasured, None),
        Verdict::Green(value) | Verdict::RedContract(value) | Verdict::RedDebt(value) | Verdict::RedOverCeiling(value) => value,
    };
    let mut points = history::points(recorded, &row.project, &row.concern.name);
    points.push(Point { at: now, value });
    let judged = trend::judge(&row.concern, value, &points, now);
    (judged.state, Some(judged))
}

fn record(rows: &[Row], history: &dyn Store, now: i64) {
    let host = host::name();
    let file = state_file("north/concerns/judged.json");
    let mut kept: BTreeMap<String, Kept> = std::fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let recorded = history.read(&host, Timestamp::from_unix_seconds(now - trend::LONGEST)).unwrap_or_else(|why| {
        eprintln!("north check concerns: concern history not read: {why}");
        Vec::new()
    });
    let samples: Vec<_> = rows
        .iter()
        .map(|row| {
            let (now_state, _) = state_of(row, &recorded, now);
            let key = format!("{}/{}", row.project, row.concern.name);
            let state = match (now_state, kept.get(&key)) {
                (Judged::Unmeasured, _) => Judged::Unmeasured,
                (raw, Some(before)) => trend::settle(before.shown, before.last, raw),
                (raw, None) => raw,
            };
            if now_state != Judged::Unmeasured {
                kept.insert(key, Kept { shown: state, last: now_state });
            }
            history::sample(row, state, &host, Timestamp::from_unix_seconds(now))
        })
        .collect();
    if let (Some(parent), Ok(text)) = (file.parent(), serde_json::to_string(&kept)) {
        let _ = std::fs::create_dir_all(parent).and_then(|()| std::fs::write(&file, text));
    }
    if let Err(why) = history.record(&samples) {
        eprintln!("north check concerns: concern history not recorded: {why}");
    }
}
