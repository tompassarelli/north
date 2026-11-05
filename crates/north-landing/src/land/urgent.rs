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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::land::queue::{Ticket, file_name, pending, write};

    #[test]
    fn an_urgent_ticket_sorts_before_older_tickets_and_after_none_of_them_once_marked() {
        let queue = tempfile::tempdir().unwrap();
        for (micros, branch) in [(1, "older"), (2, "old"), (3, "fix")] {
            write(&queue.path().join(file_name(micros, branch)), &Ticket { branch: branch.into(), head: "a".repeat(40), ..Ticket::default() }).unwrap();
        }
        let order = |queue: &Path| pending(queue).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect::<Vec<_>>();
        assert_eq!(order(queue.path()), ["older", "old", "fix"]);

        let said = mark(queue.path(), "fix", "stops the laptop overload").unwrap();

        assert!(said.contains("fix") && said.contains("goes first: stops the laptop overload"), "{said}");
        assert_eq!(order(queue.path()), ["fix", "older", "old"]);
        assert!(mark(queue.path(), "missing", "x").is_err());
        assert!(parse(&["fix".into()]).is_none(), "a reason is required");
    }
}
