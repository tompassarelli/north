//! The `north test coverage refresh --start` verb: which refresh it starts and where, leaving the unit itself to [`crate::cadence`].

use std::path::Path;
use std::time::SystemTime;

use crate::cadence::{due, start, unknown_reason};
use crate::run_selected::repository;
use crate::{handoff, map};

#[derive(Default)]
struct Request {
    unknown: Option<usize>,
    listed: Option<usize>,
    named: Option<String>,
}

impl Request {
    fn parse(arguments: &[String]) -> Option<Self> {
        let mut request = Self::default();
        let mut rest = arguments.iter();
        while let Some(flag) = rest.next() {
            match (flag.as_str(), rest.next()) {
                ("--unknown", Some(value)) => request.unknown = Some(value.parse().ok()?),
                ("--listed", Some(value)) => request.listed = Some(value.parse().ok()?),
                ("--reason", Some(text)) => request.named = Some(text.clone()),
                _ => return None,
            }
        }
        Some(request)
    }

    fn reason(self, repository: &str) -> Result<Option<String>, ()> {
        match (self.named, self.unknown, self.listed) {
            (Some(text), None, None) => Ok(Some(text)),
            (None, Some(unknown), Some(listed)) => Ok(unknown_reason(unknown, listed)),
            (None, None, None) => Ok(due(&map::path_for(repository), SystemTime::now())),
            _ => Err(()),
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(request) = Request::parse(arguments) else { return usage() };
    let root = Path::new(".");
    let repository = repository(root);
    if let Some(outcome) = handoff::forwarded(&repository, arguments) {
        return report(outcome);
    }
    match request.reason(&repository) {
        Ok(Some(reason)) => report(start(root, &reason).map(|started| format!("north test coverage: {started}"))),
        Ok(None) => 0,
        Err(()) => usage(),
    }
}

fn report(outcome: Result<String, String>) -> u8 {
    match outcome {
        Ok(line) => {
            if !line.is_empty() {
                println!("{line}");
            }
            0
        }
        Err(why) => {
            eprintln!("north test coverage refresh: {why}");
            1
        }
    }
}

fn usage() -> u8 {
    eprintln!("usage: north test coverage refresh --start [--unknown N --listed M | --reason TEXT]");
    2
}
