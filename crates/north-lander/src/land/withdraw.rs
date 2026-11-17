use clap::Parser;
use std::path::{Path, PathBuf};

use super::candidate::git;
use super::lander::state_home;
use super::queue::{self, Ticket};
use north_base::land_ticket::{self, Phase};

const USAGE: &str = "usage: north land withdraw TICKET --reason TEXT [--repo DIR] [--noticed-by WHO]";

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, args_override_self = true)]
struct Options {
    ticket: String,
    #[arg(long, allow_hyphen_values = true, default_value = "")]
    reason: String,
    #[arg(long, allow_hyphen_values = true, default_value = ".")]
    repo: PathBuf,
    #[arg(long, allow_hyphen_values = true, default_value = "coordinator")]
    noticed_by: String,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let options = Options::try_parse_from(arguments).ok()?;
    (!options.reason.trim().is_empty() && !options.noticed_by.is_empty()).then_some(options)
}

/// The queued ticket TICKET names, by file name with or without `.json` or by branch, newest first,
/// in the landing queue or this host's outbox.
pub(super) fn find(queue: &Path, wanted: &str) -> Result<(PathBuf, Ticket), String> {
    let file = if wanted.ends_with(".json") { wanted.to_owned() } else { format!("{wanted}.json") };
    let mut matches = Vec::new();
    for directory in [queue.to_path_buf(), queue.join(queue::OUTBOX)] {
        let pending = queue::pending(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        matches.extend(pending.into_iter().filter(|(path, ticket)| path.file_name().is_some_and(|name| name == file.as_str()) || ticket.branch == wanted));
    }
    matches.into_iter().max_by_key(|(path, _)| queue::queued_micros(path)).ok_or_else(|| format!("no queued ticket named {wanted} in {}", queue.display()))
}

fn withdraw(queue: &Path, options: &Options) -> Result<String, String> {
    let (path, ticket) = find(queue, &options.ticket)?;
    let verdict = format!("withdrawn by {}: {}", options.noticed_by, options.reason.trim());
    let seen = land_ticket::recorded(queue).and_then(|store| land_ticket::phase(&store, queue, &path));
    let already = |path: &Path| format!("{}: the lander is already landing it, so it cannot be withdrawn", path.display());
    if seen == Some(Phase::Checking) {
        return Err(already(&path));
    }
    let moved = match queue::withdraw_seen(&path, ticket.clone(), verdict, seen).map_err(|error| format!("{}: {error}", path.display()))? {
        Ok(moved) => moved,
        Err(Some(Phase::Checking)) => return Err(already(&path)),
        Err(now) => return Err(format!("{}: its record is {now:?}, so it cannot be withdrawn", path.display())),
    };
    Ok(format!("north land: {} ({}): withdrew it, recorded in {}", ticket.branch, ticket.head, moved.display()))
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let origin = match git(&options.repo, &["remote", "get-url", "--push", "origin"]) {
        Ok(origin) => origin,
        Err(error) => {
            eprintln!("north land withdraw: {error}");
            return 1;
        }
    };
    match withdraw(&queue::directory(&state_home(), &origin), &options) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("north land withdraw: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use north_base::land_ticket::Records;

    #[test]
    fn a_ticket_the_lander_is_checking_is_refused_a_withdrawal_and_nothing_is_recorded_or_announced() {
        let queue_dir = tempfile::tempdir().unwrap();
        let path = queue_dir.path().join(queue::file_name(1_000, "lane"));
        let ticket = Ticket { branch: "lane".into(), head: "abc".into(), session: Some("claude-x".into()), queued_micros: Some(1_000), ..Default::default() };
        queue::write(&path, &ticket).unwrap();
        land_ticket::queued(&path, "enqueued").unwrap();
        Records::of(&path).unwrap().checking(std::slice::from_ref(&path)).unwrap();
        let options = Options { ticket: "lane".into(), reason: "audit".into(), repo: ".".into(), noticed_by: "coordinator".into() };

        let refused = withdraw(queue_dir.path(), &options).unwrap_err();

        assert!(refused.contains("already landing it"), "{refused}");
        assert!(path.exists() && !queue_dir.path().join(queue::WITHDRAWN).join(path.file_name().unwrap()).exists());
    }
}
