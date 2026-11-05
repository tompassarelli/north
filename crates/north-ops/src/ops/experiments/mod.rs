//! `north experiments`: every policy and routing change judges itself on its review date. The
//! registry names each change, the outcome metric it should move and when it is reviewed;
//! `check` is the land check that keeps the rationale notes' review rules in the registry, and
//! `judge` keeps, reverts or brings each due entry to Tom with the numbers.
pub mod file;
pub mod read;
pub mod registry;
pub mod rules;
pub mod verdict;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::foundation::clock::Timestamp;
use crate::threads::calibration::rework::Sources;
use file::{Filed, GitHub, Issues};
use crate::ledger::outcome::{Outcome, read::outcomes};
use read::{Detected, Recorded};
use registry::{Day, Experiment};
use crate::threads::incidents::report::detected;
use verdict::{due, judge};

const USAGE: &str = "usage: north experiments check [--root DIR] | north experiments judge [--root DIR] [--today YYYY-MM-DD] [--file]";

struct Options {
    root: PathBuf,
    today: Day,
    file: bool,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn parse(arguments: &[String]) -> Option<Options> {
    let code = std::env::var_os("NORTH_CODE_ROOT").filter(|value| !value.is_empty()).map_or_else(|| home().join("code"), PathBuf::from);
    let mut options = Options { root: code.join("north/main"), today: Day::of_unix(Timestamp::now().unix_seconds()), file: false };
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--root" => options.root = PathBuf::from(rest.next()?),
            "--today" => options.today = Day::parse(rest.next()?)?,
            "--file" => options.file = true,
            _ => return None,
        }
    }
    Some(options)
}

fn state() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home().join(".local/state"), PathBuf::from).join("north/experiments/filed")
}

fn ledger() -> PathBuf {
    std::env::var_os("THREADS_DB").map_or_else(|| home().join(".local/state/threads/threads.db"), PathBuf::from)
}

fn load(root: &Path) -> Result<Vec<Experiment>, Vec<String>> {
    let path = root.join(registry::PATH);
    let text = std::fs::read_to_string(&path).map_err(|why| vec![format!("{}: {why}", path.display())])?;
    registry::parse(&text)
}

fn recorded() -> Result<(Vec<Outcome>, Vec<Detected>), String> {
    let path = ledger();
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("ledger {}: {why}", path.display()))?;
    let failed = |why: String| format!("ledger {}: {why}", path.display());
    let outcomes = outcomes(&db, &Sources::live()).map_err(failed)?;
    let incidents = detected(&db).map_err(|why| failed(why.to_string()))?.into_iter().map(|(class, at)| Detected { class, at }).collect();
    Ok((outcomes, incidents))
}

/// Judges every due entry over what the ledger recorded, filing each verdict once when `issues` is given.
pub fn judge_due(entries: &[Experiment], today: Day, recorded: Recorded, mut filing: Option<(&dyn Issues, &mut Filed)>) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut failed = false;
    for entry in entries.iter().filter(|entry| due(entry, today)) {
        let before = entry.before.and_then(|window| read::measure(recorded, &entry.metric, &entry.scope, window, today.unix()));
        let after = read::measure(recorded, &entry.metric, &entry.scope, entry.after, today.unix());
        let judged = judge(entry, before, after);
        let line = file::line(entry, before, after, &judged);
        lines.push(format!("{}: {} ({})", entry.id, judged.verdict.word(), file::numbers(entry, before, after, &judged)));
        if let Some((issues, filed)) = filing.as_mut() {
            match file::file(*issues, filed, entry, &line, &judged.verdict) {
                Ok(done) => lines.push(done),
                Err(why) => {
                    failed = true;
                    lines.push(format!("{}: not filed: {why}", entry.id));
                }
            }
        }
    }
    (lines, failed)
}

fn judge_command(options: &Options) -> u8 {
    let entries = match load(&options.root) {
        Ok(entries) => entries,
        Err(problems) => {
            problems.iter().for_each(|problem| eprintln!("north experiments: {problem}"));
            return 1;
        }
    };
    let mut filed = Filed::load(state());
    let pending: Vec<Experiment> = entries.into_iter().filter(|entry| due(entry, options.today) && !(options.file && filed.has(entry))).collect();
    if pending.is_empty() {
        println!("experiments: nothing due unfiled on {}", options.today.text());
        return 0;
    }
    let (outcomes, incidents) = match recorded() {
        Ok(recorded) => recorded,
        Err(why) => {
            eprintln!("north experiments: {why}");
            return 1;
        }
    };
    let (lines, failed) = judge_due(&pending, options.today, Recorded { outcomes: &outcomes, incidents: &incidents }, options.file.then_some((&GitHub as &dyn Issues, &mut filed)));
    lines.iter().for_each(|line| println!("experiments: {line}"));
    u8::from(failed)
}

pub fn run(arguments: &[String]) -> u8 {
    let (verb, rest) = match arguments.split_first() {
        Some((verb, rest)) => (verb.as_str(), rest),
        None => ("", arguments),
    };
    let Some(options) = parse(rest).filter(|_| ["check", "judge"].contains(&verb)) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if verb == "judge" {
        return judge_command(&options);
    }
    let problems = rules::check(&options.root);
    problems.iter().for_each(|problem| eprintln!("north experiments check: {problem}"));
    if problems.is_empty() {
        println!("north experiments check: every review rule in {} and every card in {} has a registry entry", rules::NOTES, rules::CARDS);
    }
    u8::from(!problems.is_empty())
}
