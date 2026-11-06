use std::fs;
use std::path::{Path, PathBuf};

use super::candidate::{self, Pick, git};
use super::checks::{self, Verdict};
use super::queue::{self, Ticket};

const USAGE: &str = "usage: north land replay --base REV [--repo DIR] [--checker PROGRAM] TICKET...";
pub const WORKTREE: &str = "lander-replay";
pub const PREFIX: &str = "replay: verdict ";

struct Options {
    repo: PathBuf,
    base: String,
    checker: Vec<String>,
    tickets: Vec<PathBuf>,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { repo: PathBuf::from("."), base: String::new(), checker: vec!["safe-push".to_owned()], tickets: Vec::new() };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--repo" => options.repo = PathBuf::from(rest.next()?),
            "--base" => options.base = rest.next()?.clone(),
            "--checker" => options.checker = vec![rest.next()?.clone()],
            flag if flag.starts_with('-') => return None,
            ticket => options.tickets.push(PathBuf::from(ticket)),
        }
    }
    (!options.base.is_empty() && !options.tickets.is_empty()).then_some(options)
}

pub fn class(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Green => "landed".to_owned(),
        Verdict::Refused(detail) => format!("failed {detail}"),
        Verdict::RedMain(detail) => format!("red-main {detail}"),
        Verdict::Transient(detail) | Verdict::BrokenBase(detail) => format!("transient {detail}"),
        Verdict::Deferred(detail) => format!("deferred {detail}"),
    }
}

fn replay(options: &Options) -> Result<String, String> {
    let checkout = super::repositories::checkout(&options.repo)?;
    let worktree = candidate::lander_worktree(&checkout, WORKTREE)?;
    candidate::ensure(&checkout, &worktree)?;
    git(&worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    let base = git(&worktree, &["rev-parse", &format!("{}^{{commit}}", options.base)])?;
    let tickets: Vec<(PathBuf, Ticket)> = options.tickets.iter().map(|path| queue::read(path).map(|ticket| (path.clone(), Ticket { verdict: None, ..ticket })).map_err(|error| error.to_string())).collect::<Result<_, _>>()?;
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let built = candidate::rebuild_recorded(&worktree, &base, &tickets, scratch.path())?;
    for pick in &built.picks {
        match pick {
            Pick::Included => {}
            Pick::Decided(verdict) => return Ok(verdict.split_whitespace().next().unwrap_or("undecided").to_owned()),
            Pick::Refused(_) => return Ok("failed".to_owned()),
            Pick::Undecided(reason) | Pick::Deferred(reason) => return Ok(format!("undecided {reason}")),
        }
    }
    if built.picks.len() != tickets.len() {
        return Ok("undecided not every lane was picked".to_owned());
    }
    let log = scratch.path().join("replay-check.log");
    let verdict = match checks::start(&checks::Launch { checker: &options.checker, offload_owner: "north-land-replay", round: "replay".to_owned(), red_fix: false, lander_slot: false }, &worktree, &base, &log, false) {
        Ok(started) => started.wait(),
        Err(verdict) => verdict,
    };
    Ok(class(&verdict))
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match replay(&options) {
        Ok(verdict) => {
            println!("{PREFIX}{verdict}");
            0
        }
        Err(error) => {
            eprintln!("north land replay: {error}");
            1
        }
    }
}

pub fn verdict_of(output: &str) -> Option<&str> {
    output.lines().rev().find_map(|line| line.strip_prefix(PREFIX)).map(str::trim)
}

pub fn recorded_class(verdict: &str) -> Option<&str> {
    let class = verdict.split_whitespace().next()?;
    matches!(class, "landed" | "failed" | "conflict").then_some(class)
}

pub fn unfit_host(recorded: &str) -> Option<String> {
    let detail = recorded.strip_prefix("failed ")?;
    let logs = detail.split_whitespace().map(|word| word.trim_end_matches([';', ',', ')'])).map(Path::new).filter(|path| path.is_absolute() && path.is_file());
    logs.filter_map(|log| fs::read_to_string(log).ok()).find_map(|text| checks::unfit_host(&text))
}

pub fn same(recorded: &str, replayed: &str) -> bool {
    recorded_class(recorded).is_some_and(|class| replayed.split_whitespace().next() == Some(class))
}

pub fn lands_what_was_refused(recorded: &str, replayed: &str) -> bool {
    recorded_class(recorded).is_some_and(|class| class != "landed") && replayed.split_whitespace().next() == Some("landed")
}
