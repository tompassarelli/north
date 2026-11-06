use std::collections::BTreeSet;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::map::{self, Test};
use crate::pick::Selection;
use crate::run_selected::{MOST_BEFORE_PUSH, north, repository, selection_between};

const USAGE: &str = "usage: north coverage miss BASE HEAD --log FILE [--map FILE]";
const FAILED: [&str; 6] = ["FAIL", "SIGSEGV", "SIGABRT", "SIGKILL", "TIMEOUT", "ABORT"];
pub const MISSED: u8 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum Agreement {
    NotJudged(String),
    Agree { failed: usize },
    Missed(Vec<Test>),
}

pub fn failed(log: &str) -> BTreeSet<Test> {
    log.lines().filter_map(failed_test).collect()
}

fn failed_test(line: &str) -> Option<Test> {
    let line = line.trim_start();
    let (status, rest) = line.split_once(' ')?;
    if !FAILED.contains(&status) {
        return None;
    }
    let rest = rest.trim_start().strip_prefix('[')?.split_once(']')?.1.trim_start();
    let rest = match rest.strip_prefix('(') {
        Some(counted) => counted.split_once(')')?.1,
        None => rest,
    };
    let words: Vec<&str> = rest.split_whitespace().collect();
    let [binary, name] = words[..] else { return None };
    Some(Test { binary: binary.to_owned(), name: name.to_owned() })
}

pub fn agreement(selection: &Selection, failed: &BTreeSet<Test>) -> Agreement {
    match selection {
        Selection::Unmapped(why) => Agreement::NotJudged(why.clone()),
        Selection::Tests { tests, added, .. } if tests.len() + added.len() > MOST_BEFORE_PUSH => Agreement::NotJudged(format!("{} tests is over the {MOST_BEFORE_PUSH} run before a push", tests.len() + added.len())),
        Selection::Tests { tests, added, .. } => {
            let ran_as_added = |test: &Test| added.iter().any(|new| test.binary.starts_with(&new.package) && test.name.contains(&new.name));
            let missed: Vec<Test> = failed.difference(tests).filter(|test| !ran_as_added(test)).cloned().collect();
            if missed.is_empty() { Agreement::Agree { failed: failed.len() } } else { Agreement::Missed(missed) }
        }
    }
}

pub fn judge(selection: &Selection, log: &str, refresh: impl FnOnce() -> Result<(), String>) -> u8 {
    match agreement(selection, &failed(log)) {
        Agreement::NotJudged(why) => {
            println!("north coverage: not judged: {why}, so the selection ran nothing before the push");
            0
        }
        Agreement::Agree { failed } => {
            println!("north coverage: agree: the selection before the push ran each of the {failed} tests that failed after it");
            0
        }
        Agreement::Missed(missed) => {
            for test in &missed {
                println!("north coverage: miss: {} {} failed after the push, and the selection before it skipped it", test.binary, test.name);
            }
            match refresh() {
                Ok(()) => println!("north coverage: refreshing the map after {} missed tests", missed.len()),
                Err(why) => eprintln!("north coverage: cannot start a refresh after the miss: {why}"),
            }
            MISSED
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let [base, head, rest @ ..] = arguments else { return usage() };
    let (mut log, mut map_path) = (None, None);
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--log", Some(path)) => log = Some(path.clone()),
            ("--map", Some(path)) => map_path = Some(path.into()),
            _ => return usage(),
        }
    }
    let Some(log) = log else { return usage() };
    let text = match fs::read_to_string(&log) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("north coverage: cannot read {log}: {error}");
            return 2;
        }
    };
    let selection = selection_between(base, head, map_path).unwrap_or_else(Selection::Unmapped);
    judge(&selection, &text, start_refresh)
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn start_refresh() -> Result<(), String> {
    let log_path = map::path_for(&repository(Path::new("."))).with_extension("refresh.log");
    if let Some(directory) = log_path.parent() {
        fs::create_dir_all(directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    }
    let log = fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|error| format!("cannot create {}: {error}", log_path.display()))?;
    let errors = log.try_clone().map_err(|error| format!("{}: {error}", log_path.display()))?;
    Command::new(north()).args(["coverage", "refresh"]).stdin(Stdio::null()).stdout(log).stderr(errors).process_group(0).spawn().map(drop).map_err(|error| format!("north coverage refresh: {error}"))
}

#[cfg(test)]
#[path = "miss_tests.rs"]
mod tests;
