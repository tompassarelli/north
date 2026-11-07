//! The hosts and local needs workers' `Stop: host NEED` reports named for a box, kept in host-needs.jsonl beside the
//! launch record: dispatch reads only the newest launch's report, so a need an earlier launch learned would drop out
//! once a later launch ended on another stop, or with no report dispatch could read. A need holds while any box
//! the stop named stays unticked word for word.
use super::issues::{Issue, IssueRef};
use super::record;
use crate::pacing::placement;
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

fn parse(text: &str) -> Vec<Learned> {
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

pub fn read(directory: &Path) -> Vec<Learned> {
    parse(&std::fs::read_to_string(directory.join(FILE)).unwrap_or_default())
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

/// The needs a launch of `issue` is placed on: those its takeable boxes name and those a worker's `Stop: host` learned for
/// boxes still unticked. Dispatch's survey and a hand launch's `north fleet launch` both read them here.
pub fn of(issue: &Issue, hosts: &[String], learned: &[Learned], at: f64) -> Vec<String> {
    let mut needs = issue.box_needs(hosts, at);
    needs.extend(holding(learned, issue, at));
    needs
}

/// The needs a launch for the one box whose text holds `box_text` is placed on: that box's own needs and those a `Stop: host`
/// learned for it. A box that matches no takeable box reads the whole issue, as `of` does.
pub fn of_box(issue: &Issue, box_text: &str, hosts: &[String], learned: &[Learned], at: f64) -> Vec<String> {
    let lines: Vec<&str> = issue.takeable_box(box_text, at).collect();
    if lines.is_empty() {
        return of(issue, hosts, learned, at);
    }
    let item = issue.reference().to_string();
    let mut needs = issue.needs_in(lines.iter().copied(), hosts);
    needs.extend(learned.iter().filter(|seen| seen.item == item && seen.boxes.iter().any(|line| lines.contains(&line.as_str()))).map(|seen| seen.need.clone()));
    needs.sort();
    needs.dedup();
    needs
}

/// The needs a hand launch on `item` is placed on, read the way dispatch reads them: the item's takeable boxes in `body`, and
/// every `Stop: host` a launch on it reported; with a `box_text` (the brief's `Box:` line) only that box's. This host learns its own launches' reports into its host-needs.jsonl, and
/// reads what dispatch learned from the ledger host, where dispatch writes it.
pub fn for_launch(item: &IssueRef, body: &str, box_text: Option<&str>, at: f64, notes: &mut Vec<String>) -> Vec<String> {
    let cfg = match placement::config() {
        Ok(cfg) => cfg,
        Err(why) => {
            notes.push(format!("{item} needs unread, so placement reads only its brief: {why}"));
            return Vec::new();
        }
    };
    let directory = record::directory();
    let records = record::read(&directory);
    let mut boxes = [Issue { repo: item.repo.clone(), number: item.number, body: body.to_owned(), ..Issue::default() }];
    super::survey::stopped(&records, &mut boxes, |launch| super::fleets::remote_json(None, &["fleet", "show", &launch.name, "--json"]), crate::fleet::registry::retired_report_here, notes);
    let launches = super::stops::last_launches(&records);
    let mut learned = learn(&directory, &boxes, |issue| launches.get(&issue.reference()).map(|launch| launch.name.clone()), at).unwrap_or_else(|why| {
        notes.push(format!("host-needs.jsonl unwritten, so this launch reads only the needs already in it: {why}"));
        read(&directory)
    });
    learned.extend(from_ledger_host(item, notes));
    let [issue] = boxes;
    let hosts = super::fleets::hosts(&cfg);
    match box_text {
        Some(text) => of_box(&issue, text, &hosts, &learned, at),
        None => of(&issue, &hosts, &learned, at),
    }
}

const SERVED: [&str; 3] = ["north", "dispatch", "needs"];

fn from_ledger_host(item: &IssueRef, notes: &mut Vec<String>) -> Vec<Learned> {
    let reference = item.to_string();
    let words: Vec<&str> = SERVED.iter().copied().chain([reference.as_str()]).collect();
    match crate::threads::client::ask_ledger_host(&words, &[]) {
        Ok(Some(answer)) => parse(&String::from_utf8_lossy(&answer)),
        Ok(None) => Vec::new(),
        Err(why) => {
            notes.push(format!("the ledger host's host needs for {item} are unread, so this launch reads only this host's: {}", why.user_message()));
            Vec::new()
        }
    }
}

/// `north dispatch needs REPO#N`: one record line per need learned for the item on this host, which a launch on another host reads.
pub fn serve(item: &str) -> u8 {
    let Some(item) = IssueRef::parse(item) else {
        eprintln!("north dispatch needs: {item} names no issue");
        return 2;
    };
    let reference = item.to_string();
    for learned in read(&record::directory()).iter().filter(|learned| learned.item == reference) {
        if let Ok(line) = serde_json::to_string(learned) {
            println!("{line}");
        }
    }
    0
}
