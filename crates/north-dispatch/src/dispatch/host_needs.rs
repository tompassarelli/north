//! The hosts and local needs workers recorded for a box with `north fleet need HOST REASON`, kept in host-needs.jsonl beside the
//! launch record on the host that holds it, so a need an earlier launch recorded still places a later launch after another
//! launch ended on another stop. A need holds while any box it was recorded for stays unticked word for word.
use super::issues::{Issue, IssueRef};
use super::stops::Relay;
use super::record;
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
    /// Why the worker needs it, as it gave it to `north fleet need`.
    #[serde(default)]
    pub reason: String,
}

fn parse(text: &str) -> Vec<Learned> {
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

pub fn read(directory: &Path) -> Vec<Learned> {
    parse(&std::fs::read_to_string(directory.join(FILE)).unwrap_or_default())
}

/// Whether `need` reads as a host name or a local need word: lower case, starting with a letter.
pub fn is_need(need: &str) -> bool {
    need.chars().next().is_some_and(|first| first.is_ascii_lowercase()) && need.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// Appends the need `session` recorded for `issue` with `north fleet need`, once per session and need, and returns every record including it.
/// The boxes are the one box the launch staffed when `box_text` names a takeable one, else the issue's unticked takeable ones, so the need holds while any stays unticked.
pub fn record(directory: &Path, issue: &Issue, box_text: Option<&str>, session: &str, need: &str, reason: &str, at: f64) -> Result<Vec<Learned>, String> {
    let mut known = read(directory);
    let item = issue.reference().to_string();
    if known.iter().any(|seen| seen.item == item && seen.need == need && seen.session == session) {
        return Ok(known);
    }
    let named: Vec<String> = box_text.map(|text| issue.takeable_box(text, at).map(|line| line.trim().to_owned()).collect()).unwrap_or_default();
    let boxes = if named.is_empty() { issue.takeable(at).map(|line| line.trim().to_owned()).collect() } else { named };
    let learned = Learned { item, need: need.to_owned(), session: session.to_owned(), at, boxes, reason: reason.trim().to_owned() };
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(FILE)).map_err(|error| error.to_string())?;
    let line = format!("{}\n", serde_json::to_string(&learned).map_err(|error| error.to_string())?);
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())?;
    known.push(learned);
    Ok(known)
}

/// The stopped run the next launch of `item` follows: the newest launch's session, when it recorded a need, with its reason as the report the brief carries.
pub fn relay(learned: &[Learned], item: &str, session: &str) -> Option<Relay> {
    let seen = learned.iter().rfind(|seen| seen.item == item && seen.session == session)?;
    Some(Relay { session: seen.session.clone(), need: seen.need.clone(), report: seen.reason.clone() })
}

/// Records the need the calling session found, on the host that holds the dispatch records: the item is the one given, else the one `session`'s launch record names.
pub fn register(session: &str, need: &str, reason: &str, item: Option<&str>) -> Result<String, String> {
    if !is_need(need) {
        return Err(format!("{need} is not a host or need name: lower case letters, digits, '.', '_' and '-', starting with a letter"));
    }
    let directory = record::directory();
    let launched = record::read(&directory).into_iter().rfind(|launch| launch.launched && launch.name == session);
    let item = item.map(str::to_owned).or_else(|| launched.as_ref().map(|launch| launch.item.clone())).ok_or_else(|| format!("no launch of session {session} is on record here, so name its issue with --item REPO#N"))?;
    let at = crate::pacing::placement::now();
    let reference = IssueRef::parse(&item).ok_or_else(|| format!("{item} names no issue"))?;
    let (body, _) = super::manual::issue_text(&item)?;
    let issue = Issue { repo: reference.repo, number: reference.number, body, ..Issue::default() };
    let box_text = launched.filter(|launch| launch.item == item).and_then(|launch| launch.box_text);
    record(&directory, &issue, box_text.as_deref(), session, need, reason, at)?;
    Ok(format!("recorded: {session} needs {need} for {item}; the next try of its boxes is placed there"))
}

/// The needs a launch of `issue` is placed on: those a worker recorded with `north fleet need` for boxes still unticked, and those every takeable box shares, each box
/// read as its own `Needs:` line says, a box with none needing nothing, whatever its prose mentions. One box that needs nothing places the launch, because a worker takes the boxes it can and
/// records the need for the rest, which a held launch would lose. Dispatch's survey and a hand launch's `north fleet launch` both read them here.
pub fn of(issue: &Issue, learned: &[Learned], at: f64) -> Vec<String> {
    of_with(issue, learned, at, &repo_lander_host)
}

pub fn of_with(issue: &Issue, learned: &[Learned], at: f64, lander: &dyn Fn(&IssueRef) -> Option<String>) -> Vec<String> {
    placed(issue, issue.takeable(at), learned, lander)
}

fn placed<'a>(issue: &Issue, lines: impl Iterator<Item = &'a str>, learned: &[Learned], lander: &dyn Fn(&IssueRef) -> Option<String>) -> Vec<String> {
    let item = issue.reference();
    let lines: Vec<&str> = lines.collect();
    let mut needs: Vec<String> = learned.iter().filter(|seen| seen.item == item.to_string() && seen.boxes.iter().any(|boxed| lines.iter().any(|line| line.trim() == boxed))).map(|seen| seen.need.clone()).collect();
    let mut shared: Option<Vec<String>> = None;
    for line in &lines {
        let own = issue.needs_in(std::iter::once(*line));
        shared = Some(shared.map_or(own.clone(), |before| before.into_iter().filter(|need| own.contains(need)).collect()));
    }
    needs.extend(shared.unwrap_or_default());
    let needs = needs.into_iter().map(|need| if need == crate::pacing::placement::needs::LANDER { lander(&item).unwrap_or(need) } else { need });
    let mut needs: Vec<String> = needs.collect();
    needs.sort();
    needs.dedup();
    needs
}

/// The needs a launch for the one box whose text holds `box_text` is placed on: that box's own, read as `of` reads each box. A box that matches no takeable box reads the whole issue, as `of` does.
pub fn of_box(issue: &Issue, box_text: &str, learned: &[Learned], at: f64) -> Vec<String> {
    of_box_with(issue, box_text, learned, at, &repo_lander_host)
}

pub fn of_box_with(issue: &Issue, box_text: &str, learned: &[Learned], at: f64, lander: &dyn Fn(&IssueRef) -> Option<String>) -> Vec<String> {
    let lines: Vec<&str> = issue.takeable_box(box_text, at).collect();
    if lines.is_empty() {
        return of_with(issue, learned, at, lander);
    }
    placed(issue, lines.into_iter(), learned, lander)
}

fn repo_lander_host(item: &IssueRef) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let checkout = std::path::Path::new(&home).join("code").join(crate::threads::catalog::checkout(&item.repo)).join("main");
    let directory = checkout.to_str()?;
    let output = crate::platform::git::run_within(&["-C", directory, "config", "--blob", "refs/remotes/origin/main:.safe-push", "--get", "landing.lander-host"], super::fleets::PROBE)?;
    let host = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !host.is_empty()).then_some(host)
}

/// The needs a hand launch on `item` is placed on, read the way dispatch reads them: the item's takeable boxes in `body`, and
/// every need a launch on it recorded; with a `box_text` (the brief's `Box:` line) only that box's. This host reads its own host-needs.jsonl and
/// the ledger host's, where a worker's `north fleet need` records.
pub fn for_launch(item: &IssueRef, body: &str, box_text: Option<&str>, at: f64, notes: &mut Vec<String>) -> Vec<String> {
    let issue = Issue { repo: item.repo.clone(), number: item.number, body: body.to_owned(), ..Issue::default() };
    let mut learned = read(&record::directory());
    learned.extend(from_ledger_host(item, notes));
    match box_text {
        Some(text) => of_box(&issue, text, &learned, at),
        None => of(&issue, &learned, at),
    }
}

const SERVED: [&str; 4] = ["north", "fleet", "dispatch", "needs"];

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

/// `north fleet dispatch needs REPO#N`: one record line per need learned for the item on this host, which a launch on another host reads.
pub fn serve(item: &str) -> u8 {
    let Some(item) = IssueRef::parse(item) else {
        eprintln!("north fleet dispatch needs: {item} names no issue");
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

/// `north fleet dispatch need SESSION NEED [REPO#N]`, the reason on stdin: what `north fleet need` asks the host holding the dispatch records to write.
pub fn serve_need(session: &str, need: &str, item: Option<&str>) -> u8 {
    let mut reason = String::new();
    if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut reason) {
        eprintln!("north fleet dispatch need: the reason is unread: {error}");
        return 2;
    }
    match register(session, need, &reason, item) {
        Ok(recorded) => {
            println!("{recorded}");
            0
        }
        Err(why) => {
            eprintln!("north fleet dispatch need: {why}");
            1
        }
    }
}
