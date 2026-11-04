use std::path::{Path, PathBuf};
use std::process::Command;

use super::after::{self, follow};
use super::candidate::git;
use super::incidents::{self, Noticed};
use super::lander::state_home;
use super::queue::{self, Ticket};

const USAGE: &str = "usage: north land withdraw TICKET --reason TEXT [--repo DIR] [--noticed-by WHO] [--class CLASS]";
const AREA: &str = "withdrawn";

struct Options {
    ticket: String,
    reason: String,
    repo: PathBuf,
    noticed_by: String,
    class: String,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { ticket: String::new(), reason: String::new(), repo: PathBuf::from("."), noticed_by: "proxy".into(), class: "lander-withdrawn".into() };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--reason" => options.reason.clone_from(rest.next()?),
            "--repo" => options.repo = PathBuf::from(rest.next()?),
            "--noticed-by" => options.noticed_by.clone_from(rest.next()?),
            "--class" => options.class.clone_from(rest.next()?),
            word if word.starts_with('-') || !options.ticket.is_empty() => return None,
            word => options.ticket = word.to_owned(),
        }
    }
    (!options.ticket.is_empty() && !options.reason.trim().is_empty() && !options.noticed_by.is_empty() && !options.class.is_empty()).then_some(options)
}

/// The queued ticket TICKET names, by file name with or without `.json` or by branch, newest first,
/// in the landing queue or this host's outbox.
fn find(queue: &Path, wanted: &str) -> Result<(PathBuf, Ticket), String> {
    let file = if wanted.ends_with(".json") { wanted.to_owned() } else { format!("{wanted}.json") };
    let mut matches = Vec::new();
    for directory in [queue.to_path_buf(), queue.join(queue::OUTBOX)] {
        for (path, ticket) in queue::pending(&directory).map_err(|error| format!("{}: {error}", directory.display()))? {
            if path.file_name().is_some_and(|name| name == file.as_str()) || ticket.branch == wanted {
                matches.push((path, ticket));
            }
        }
    }
    matches.into_iter().max_by_key(|(path, _)| queue::queued_micros(path)).ok_or_else(|| format!("no queued ticket named {wanted} in {}", queue.display()))
}

fn withdraw(queue: &Path, options: &Options) -> Result<String, String> {
    let (path, ticket) = find(queue, &options.ticket)?;
    let verdict = format!("withdrawn by {}: {}", options.noticed_by, options.reason.trim());
    let moved = queue::settle(&path, ticket.clone(), verdict.clone(), AREA).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut lines = vec![format!("north land: withdrew {} ({}) to {}", ticket.branch, ticket.head, moved.display())];
    if let Some(session) = ticket.session.as_deref().filter(|session| !session.is_empty()) {
        let told = after::send(session, &format!("north land: {} ({}) was withdrawn from the landing queue: {}", ticket.branch, ticket.head, options.reason.trim()));
        lines.push(if told { format!("north land: told session {session}") } else { format!("north land: could not tell session {session}") });
    }
    let noticed = Noticed { class: &options.class, key: format!("withdrawn:{}@{}", moved.file_name().unwrap_or_default().to_string_lossy(), ticket.head), noticed_by: &options.noticed_by, detail: &verdict };
    let arguments = incidents::arguments(&path, &ticket, &noticed, super::telemetry::epoch());
    let recorded = Command::new(follow::program()).args(&arguments).status().map_err(|error| format!("{} incident record: {error}", follow::program()))?;
    lines.push(if recorded.success() { format!("north land: recorded incident {}", noticed.key) } else { format!("north land: incident record exited {recorded}; the ticket stays withdrawn") });
    Ok(lines.join("\n"))
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

    fn ticket(branch: &str) -> Ticket {
        Ticket {
            origin: "https://github.com/tompassarelli/north.git".into(),
            root: PathBuf::from("/lanes/a"),
            host: "laptop".into(),
            branch: branch.into(),
            head: "f".repeat(40),
            session: None,
            queued_at: "2026-11-04T03:00:00Z".into(),
            first_commit_secs: None,
            several_commits: None,
            red_fix: None,
            verdict: None,
            keep_lane: false,
        }
    }

    #[test]
    fn a_ticket_is_found_by_file_or_branch_in_the_queue_or_the_outbox_newest_first() {
        let queue = tempfile::tempdir().unwrap();
        queue::write(&queue.path().join("100-a.json"), &ticket("a")).unwrap();
        queue::write(&queue.path().join("outbox/200-a.json"), &ticket("a")).unwrap();
        queue::write(&queue.path().join("300-b.json"), &ticket("b")).unwrap();
        assert_eq!(find(queue.path(), "a").unwrap().0, queue.path().join("outbox/200-a.json"));
        assert_eq!(find(queue.path(), "100-a").unwrap().0, queue.path().join("100-a.json"));
        assert_eq!(find(queue.path(), "300-b.json").unwrap().1.branch, "b");
        assert!(find(queue.path(), "c").is_err());
    }

    #[test]
    fn withdraw_needs_a_ticket_and_a_reason() {
        let words = |text: &str| text.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        assert!(parse(&words("a")).is_none());
        assert!(parse(&words("--reason x")).is_none());
        assert!(parse(&words("a b --reason x")).is_none());
        let options = parse(&words("a --reason superseded --noticed-by tom")).unwrap();
        assert_eq!((options.ticket.as_str(), options.reason.as_str(), options.noticed_by.as_str(), options.class.as_str()), ("a", "superseded", "tom", "lander-withdrawn"));
    }
}
