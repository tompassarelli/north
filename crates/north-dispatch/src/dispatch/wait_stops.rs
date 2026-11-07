//! The `Stop: wait until T` reports workers ended on, kept in wait-stops.jsonl beside the launch record: dispatch reads only
//! the newest launch's report, so a wait an earlier launch learned would drop out once a later launch ended on another stop,
//! or with no report dispatch could read. A wait holds until T while any box the stop could have taken stays unticked word for word.
use super::issues::Issue;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

const FILE: &str = "wait-stops.jsonl";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Learned {
    pub item: String,
    /// The time the report named, as written.
    pub until: String,
    pub until_at: f64,
    pub session: String,
    pub at: f64,
    /// The unticked product boxes the stopped session could take.
    pub boxes: Vec<String>,
}

pub fn read(directory: &Path) -> Vec<Learned> {
    std::fs::read_to_string(directory.join(FILE)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Records the `Stop: wait until T` each issue's newest launch reported while T is ahead of `at`, once per session and time,
/// and returns every record including the new ones.
pub fn learn(directory: &Path, issues: &[Issue], session_of: impl Fn(&Issue) -> Option<String>, at: f64) -> Result<Vec<Learned>, String> {
    let mut known = read(directory);
    let mut fresh = String::new();
    for issue in issues {
        let (Some((until, until_at)), Some(session)) = (issue.stop.until.clone().filter(|(_, until_at)| *until_at > at), session_of(issue)) else { continue };
        let item = issue.reference().to_string();
        if known.iter().any(|seen| seen.item == item && seen.until == until && seen.session == session) {
            continue;
        }
        let learned = Learned { item, until, until_at, session, at, boxes: issue.takeable(at).map(|line| line.trim().to_owned()).collect() };
        fresh.push_str(&format!("{}\n", serde_json::to_string(&learned).map_err(|error| error.to_string())?));
        known.push(learned);
    }
    if !fresh.is_empty() {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(FILE)).map_err(|error| error.to_string())?;
        file.write_all(fresh.as_bytes()).map_err(|error| error.to_string())?;
    }
    Ok(known)
}

/// The latest time ahead of `at` that a learned wait holds `issue` until, as written and as unix seconds: a wait counts while
/// one of its boxes is still an unticked takeable box of the issue.
pub fn holding(learned: &[Learned], issue: &Issue, at: f64) -> Option<(String, f64)> {
    let item = issue.reference().to_string();
    let open: Vec<&str> = issue.takeable(at).map(str::trim).collect();
    learned
        .iter()
        .filter(|seen| seen.item == item && seen.until_at > at && seen.boxes.iter().any(|line| open.contains(&line.as_str())))
        .max_by(|a, b| a.until_at.total_cmp(&b.until_at))
        .map(|seen| (seen.until.clone(), seen.until_at))
}
