use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::records_mirror;

use super::candidate::git;
use super::queue::{self, Ticket};

const USAGE: &str = "usage: north land status TICKET [--repo DIR]";

struct Options {
    ticket: String,
    repo: PathBuf,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { ticket: String::new(), repo: PathBuf::from(".") };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--repo" => options.repo = PathBuf::from(rest.next()?),
            word if word.starts_with('-') || !options.ticket.is_empty() => return None,
            word => options.ticket = word.to_owned(),
        }
    }
    (!options.ticket.is_empty()).then_some(options)
}

fn areas(queue: &Path) -> [PathBuf; 6] {
    [queue.join(queue::OUTBOX), queue.to_owned(), queue.join("done"), queue.join(queue::QUARANTINE), queue.join(queue::WITHDRAWN), queue.join(queue::OUTBOX).join(queue::DECIDED)]
}

fn find(queue: &Path, wanted: &str) -> Option<(PathBuf, Ticket)> {
    let file = if wanted.ends_with(".json") { wanted.to_owned() } else { format!("{wanted}.json") };
    let found = areas(queue)
        .into_iter()
        .enumerate()
        .flat_map(|(rank, directory)| {
            fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
                .filter(|path| path.file_name().is_some_and(|name| name == file.as_str()) || queue::read(path).is_ok_and(|ticket| ticket.branch == wanted))
                .filter_map(|path| queue::read(&path).ok().map(|ticket| (rank, path, ticket)))
                .collect::<Vec<_>>()
        })
        .max_by_key(|(rank, path, _)| (queue::queued_micros(path), *rank))?;
    Some((found.1, found.2))
}

fn queued(queue: &Path, path: &Path, ticket: &Ticket) -> String {
    let place = queue::queue_position(queue, path).ok().flatten().map_or_else(|| "waiting".to_owned(), |(place, total)| format!("{place} of {total} queued"));
    match ticket.deferral.as_deref().filter(|reason| !reason.trim().is_empty()) {
        Some(reason) => format!("north land: {} ({}) is {place}; {reason}", ticket.branch, ticket.head),
        None => format!("north land: {} ({}) is {place}", ticket.branch, ticket.head),
    }
}

fn decided(area: &str, ticket: &Ticket) -> String {
    let verdict = ticket.verdict.as_deref().unwrap_or("undecided");
    format!("north land: {} ({}) is {area}: {verdict}", ticket.branch, ticket.head)
}

pub fn report(home: &Path, origin: &str, wanted: &str) -> Result<String, String> {
    let queue = queue::directory(home, origin);
    let (path, ticket) = find(&queue, wanted).ok_or_else(|| format!("no ticket named {wanted} in {}", queue.display()))?;
    let area = path.parent().filter(|parent| *parent != queue).and_then(Path::file_name).and_then(|name| name.to_str());
    Ok(match area {
        Some(queue::WITHDRAWN) => decided("withdrawn", &ticket),
        Some(queue::QUARANTINE) => decided("quarantined", &ticket),
        Some("done" | queue::DECIDED) => decided("decided", &ticket),
        Some(queue::OUTBOX) => format!("north land: {} ({}) is waiting in this host's outbox", ticket.branch, ticket.head),
        _ => queued(&queue, &path, &ticket),
    })
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let records = records_mirror::read("north land status");
    let local = records_mirror::state_home();
    let reported = git(&options.repo, &["remote", "get-url", "--push", "origin"]).and_then(|origin| report(records.home(), &origin, &options.ticket).or_else(|mirrored| report(&local, &origin, &options.ticket).map_err(|_| mirrored)));
    match reported {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("north land status: {error}");
            1
        }
    }
}
