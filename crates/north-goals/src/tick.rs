//! The fleet tick's goal line: the proxy hears of red goals the tick after they turn red, again when
//! the set of red goals changes and as a reminder every few hours, and a tick reads the oracles at most every few minutes.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{PROJECTS, Row, check, code_root, line};

const CHECK_EVERY: i64 = 600;
const REMIND_EVERY: i64 = 6 * 3600;

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
    let set = bad.iter().map(|row| format!("{}/{}:{}", row.project, row.goal.name, row.verdict.word())).chain(problems.iter().cloned()).collect::<Vec<_>>().join(";");
    let lines: Vec<String> = bad.iter().map(|row| line(row)).chain(problems.iter().map(|problem| format!("not read: {problem}"))).collect();
    Red { set, text: format!("goals red ({}): {}", lines.len(), lines.join("; ")) }
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

pub fn lines(now: i64) -> Vec<String> {
    announce(&state_file("north/goals/tick.json"), now, || {
        let projects: Vec<String> = PROJECTS.iter().map(|name| (*name).to_owned()).collect();
        let (rows, problems) = check(&code_root(), &projects);
        red(&rows, &problems)
    })
}

pub fn announce(file: &Path, now: i64, measure: impl FnOnce() -> Red) -> Vec<String> {
    let state = std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let (told, state) = decide(state, now, measure);
    if let (Some(parent), Ok(text)) = (file.parent(), serde_json::to_string(&state)) {
        let _ = std::fs::create_dir_all(parent).and_then(|()| std::fs::write(file, text));
    }
    told.into_iter().collect()
}
