use std::collections::BTreeMap;
use std::fmt::Write;

use north_github_mirror::store::Mirror;
use rusqlite::{Connection, params};

use super::{Sources, duration, open, workers};
use crate::foundation::clock::Timestamp;
use crate::threads::attribution::{Basis, Cause};

const USAGE: &str = "usage: north dash losses [--since 24h] [--until DATE] [--repo north]";
const DEFAULT_SINCE: i64 = 86_400;

struct Request {
    since_s: i64,
    until: Option<i64>,
    repo: String,
}

impl Request {
    fn parse(arguments: &[String]) -> Option<Self> {
        let mut request = Self { since_s: DEFAULT_SINCE, until: None, repo: "north".to_owned() };
        let mut words = arguments.iter();
        while let Some(flag) = words.next() {
            let value = words.next()?;
            match flag.as_str() {
                "--since" => request.since_s = span_seconds(value)?,
                "--until" => request.until = Some(Timestamp::parse(value).ok()?.unix_seconds()),
                "--repo" => request.repo = value.clone(),
                _ => return None,
            }
        }
        Some(request)
    }

    fn window(&self, now: i64) -> (i64, i64) {
        let end = self.until.unwrap_or(now);
        (end - self.since_s, end)
    }
}

fn span_seconds(text: &str) -> Option<i64> {
    let at = text.find(|c: char| !c.is_ascii_digit())?;
    let (count, unit) = text.split_at(at);
    let count: i64 = count.parse().ok()?;
    Some(count * match unit {
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return None,
    })
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(request) = Request::parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match read(&Sources::live(), &request, Timestamp::now().unix_seconds()) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(why) => {
            eprintln!("north dash losses: {why}");
            1
        }
    }
}

fn read(sources: &Sources, request: &Request, now: i64) -> Result<String, String> {
    let threads = open(&sources.threads)?;
    let (start, end) = request.window(now);
    let attempts = workers::attempts(&threads, start, &workers::missed_checks(&Mirror::local()), &workers::Pending::read(&threads, now)).map_err(|why| format!("{}: {why}", sources.threads.display()))?;
    Ok(render(&threads, &attempts, request, start, end))
}

fn render(threads: &Connection, attempts: &[workers::Attempt], request: &Request, start: i64, end: i64) -> String {
    let held: Vec<&workers::Attempt> = attempts.iter().filter(|attempt| (start..end).contains(&attempt.ended) && attempt.repos.contains(&request.repo)).collect();
    let mut lost: Vec<&workers::Attempt> = held.iter().copied().filter(|attempt| matches!(attempt.cause, Cause::Harness(_))).collect();
    lost.sort_by_key(|attempt| attempt.ended);
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for attempt in &lost {
        *classes.entry(attempt.cause.label()).or_default() += 1;
    }
    let mut ranked: Vec<(&str, usize)> = classes.into_iter().collect();
    ranked.sort_by_key(|(class, count)| (std::cmp::Reverse(*count), *class));
    let mut text = format!("{}: {} of {} first tries lost to the harness in {} to {}\n", request.repo, lost.len(), held.len(), duration(request.since_s as f64), Timestamp::from_unix_seconds(end).iso());
    let _ = writeln!(text, "classes: {}", ranked.iter().map(|(class, count)| format!("{class} {count}")).collect::<Vec<_>>().join(", "));
    for attempt in lost {
        let _ = writeln!(text, "{} {} {} [{}] {}", Timestamp::from_unix_seconds(attempt.ended).iso(), or_dash(&attempt.session), or_dash(&attempt.item), attempt.cause.label(), evidence(threads, attempt));
    }
    text
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() { "-" } else { text }
}

fn evidence(threads: &Connection, attempt: &workers::Attempt) -> String {
    if let Some(recorded) = boundary(threads, attempt) {
        return recorded;
    }
    match attempt.basis {
        Basis::Recorded => "the ledger recorded its cause with no boundary row".into(),
        Basis::Unclaimed => "no boundary row; the run is the worker's".into(),
        Basis::Done => "done".into(),
    }
}

fn boundary(threads: &Connection, attempt: &workers::Attempt) -> Option<String> {
    let started = Timestamp::from_unix_seconds(attempt.started).iso();
    let mut statement = threads.prepare("SELECT source, cause, at, reason FROM boundary_causes WHERE session = ?1 AND source != 'resume' AND at >= ?2 ORDER BY at DESC, rowid DESC LIMIT 1").ok()?;
    let (source, cause, at, reason): (String, String, String, String) = statement.query_row(params![attempt.session, started], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).ok()?;
    let at = Timestamp::parse(&at).map_or(at, |at| at.iso());
    let recorded = format!("recorded by {source} as {cause} at {at}");
    Some(if reason.is_empty() { recorded } else { format!("{recorded}: {reason}") })
}

#[cfg(test)]
#[path = "losses_tests.rs"]
mod tests;
