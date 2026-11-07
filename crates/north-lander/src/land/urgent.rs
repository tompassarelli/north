use std::path::{Path, PathBuf};

use super::candidate::git;
use super::lander::state_home;
use super::queue;
use super::withdraw::find;

pub const USAGE: &str = "usage: north land urgent TICKET --reason TEXT [--repo DIR]";

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

pub fn mark(queue: &Path, wanted: &str, reason: &str) -> Result<String, String> {
    let (path, mut ticket) = find(queue, wanted)?;
    ticket.urgent = Some(reason.trim().to_owned());
    queue::write(&path, &ticket).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("north land: {} ({}) goes first: {}", ticket.branch, ticket.head, reason.trim()))
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let marked = git(&options.repo, &["remote", "get-url", "--push", "origin"]).and_then(|origin| mark(&queue::directory(&state_home(), &origin), &options.ticket, &options.reason));
    match marked {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("north land urgent: {error}");
            1
        }
    }
}
