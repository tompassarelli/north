use clap::Parser;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::candidate::git;
use super::lander::state_home;
use super::queue;
use super::withdraw::find;

pub const USAGE: &str = "usage: north land urgent TICKET --reason TEXT [--repo DIR]";

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, args_override_self = true)]
struct Options {
    ticket: String,
    #[arg(long, allow_hyphen_values = true, default_value = "")]
    reason: String,
    #[arg(long, allow_hyphen_values = true, default_value = ".")]
    repo: PathBuf,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let options = Options::try_parse_from(arguments).ok()?;
    (!options.reason.trim().is_empty()).then_some(options)
}

pub fn mark(queue: &Path, wanted: &str, reason: &str) -> Result<String, String> {
    let (path, mut ticket) = find(queue, wanted)?;
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_micros();
    ticket.urgent = Some(reason.trim().to_owned());
    ticket.urgent_at = Some(at);
    queue::write(&path, &ticket).map_err(|error| format!("{}: {error}", path.display()))?;
    let directory = path.parent().ok_or_else(|| format!("{} has no queue", path.display()))?;
    let (place, total) = queue::queue_position(directory, &path).map_err(|error| format!("{}: {error}", directory.display()))?.ok_or_else(|| format!("{} is not queued in {}", path.display(), directory.display()))?;
    Ok(format!("north land: {} ({}) is {place} of {total} queued: {}", ticket.branch, ticket.head, reason.trim()))
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

#[cfg(test)]
#[path = "urgent_tests.rs"]
mod tests;
