use std::path::{Path, PathBuf};

use super::candidate::git;
use super::lander::state_home;
use super::queue::{self, Ticket};
use super::queue::QUARANTINE;

const USAGE: &str = "usage: north land requeue TICKET --reason TEXT [--repo DIR]";

struct Options {
    ticket: String,
    reason: String,
    repo: PathBuf,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { ticket: String::new(), reason: String::new(), repo: PathBuf::from(".") };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--reason" => options.reason.clone_from(rest.next()?),
            "--repo" => options.repo = PathBuf::from(rest.next()?),
            word if word.starts_with('-') || !options.ticket.is_empty() => return None,
            word => options.ticket = word.to_owned(),
        }
    }
    (!options.ticket.is_empty() && !options.reason.trim().is_empty()).then_some(options)
}

fn quarantined(queue: &Path, wanted: &str) -> Result<(PathBuf, Ticket), String> {
    let file = if wanted.ends_with(".json") { wanted.to_owned() } else { format!("{wanted}.json") };
    let directory = queue.join(QUARANTINE);
    let found = queue::pending(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    found
        .into_iter()
        .filter(|(path, ticket)| path.file_name().is_some_and(|name| name == file.as_str()) || ticket.branch == wanted)
        .max_by_key(|(path, _)| queue::queued_micros(path))
        .ok_or_else(|| format!("no quarantined ticket named {wanted} in {}", directory.display()))
}

/// Puts the quarantined ticket TICKET names back in the queue for another check, recording `reason` as the move's cause.
pub fn requeue(queue: &Path, wanted: &str, reason: &str) -> Result<String, String> {
    let (path, ticket) = quarantined(queue, wanted)?;
    let cause = format!("requeued from quarantine: {}", reason.trim());
    let queued = queue::requeue(queue, &path, &cause).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("north land: {} ({}) is queued again in {}", ticket.branch, ticket.head, queued.display()))
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let queued = git(&options.repo, &["remote", "get-url", "--push", "origin"]).and_then(|origin| requeue(&queue::directory(&state_home(), &origin), &options.ticket, &options.reason));
    match queued {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("north land requeue: {error}");
            1
        }
    }
}
