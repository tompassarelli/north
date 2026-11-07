//! The hosts and local needs workers' `Stop: host NEED` reports named for a box, kept in host-needs.jsonl beside the
//! launch record: dispatch reads only the newest launch's report, so a need an earlier launch learned would drop out
//! once a later launch ended on another stop, or with no report dispatch could read. A need holds while any box
//! the stop named stays unticked word for word.
use super::issues::Issue;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

const FILE: &str = "host-needs.jsonl";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Learned {
    pub item: String,
    pub need: String,
    pub session: String,
    pub at: f64,
    /// The unticked product boxes the stopped session could take.
    pub boxes: Vec<String>,
}

pub fn read(directory: &Path) -> Vec<Learned> {
    std::fs::read_to_string(directory.join(FILE)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Records the `Stop: host` need each issue's newest launch reported, once per session and need, and returns every
/// record including the new ones.
pub fn learn(directory: &Path, issues: &[Issue], session_of: impl Fn(&Issue) -> Option<String>, at: f64) -> Result<Vec<Learned>, String> {
    let mut known = read(directory);
    let mut fresh = String::new();
    for issue in issues {
        let (Some(need), Some(session)) = (issue.stop.host.clone(), session_of(issue)) else { continue };
        let item = issue.reference().to_string();
        if known.iter().any(|seen| seen.item == item && seen.need == need && seen.session == session) {
            continue;
        }
        let learned = Learned { item, need, session, at, boxes: issue.takeable(at).map(|line| line.trim().to_owned()).collect() };
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

/// The needs learned for `issue` whose boxes it still holds unticked.
pub fn holding(learned: &[Learned], issue: &Issue, at: f64) -> Vec<String> {
    let item = issue.reference().to_string();
    let open: Vec<&str> = issue.takeable(at).map(str::trim).collect();
    let mut needs: Vec<String> = learned.iter().filter(|seen| seen.item == item && seen.boxes.iter().any(|line| open.contains(&line.as_str()))).map(|seen| seen.need.clone()).collect();
    needs.sort();
    needs.dedup();
    needs
}
