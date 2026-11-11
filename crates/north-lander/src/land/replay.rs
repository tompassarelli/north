use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::candidate::{self, Pick, git};
use super::hold::{self, Locked};
use super::checks::{self, Verdict};
use super::queue::{self, Ticket};
use crate::landing::red_check::{self, Failure};

const USAGE: &str = "usage: north land replay --base REV [--repo DIR] [--checker PROGRAM] TICKET...";
pub const WORKTREE: &str = "lander-replay";
const WORKTREES: usize = 4;
pub const PREFIX: &str = "replay: verdict ";
const FAILURE: &str = "replay: failure ";

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

pub struct Replayed {
    pub verdict: String,
    pub failures: BTreeSet<Failure>,
}

impl Replayed {
    fn of(verdict: impl Into<String>) -> Self {
        Self { verdict: verdict.into(), failures: BTreeSet::new() }
    }
}

/// The commit a recorded round landed as, when every ticket records a landed tip: the tip every other tip
/// descends from, which is the candidate the round pushed. A replay of that round checks that candidate
/// rather than rebuilding it from the lane heads, so the generated registries the lander wrote at land time
/// are already in it and a replay never depends on a generator that no longer covers the round's tree.
pub fn landed_tip(verdict: &str) -> Option<&str> {
    let tip = verdict.strip_prefix("landed ")?.split_whitespace().next()?;
    (tip.len() == 40 && tip.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(tip)
}

fn landed_candidate(worktree: &Path, base: &str, tickets: &[(PathBuf, Ticket)]) -> Option<String> {
    let tips: Vec<&str> = tickets.iter().filter_map(|(_, ticket)| landed_tip(ticket.verdict.as_deref()?)).collect();
    if tips.is_empty() {
        return None;
    }
    let head = tips.iter().find(|candidate| tips.iter().all(|other| *other == **candidate || git(worktree, &["merge-base", "--is-ancestor", other, candidate]).is_ok()))?;
    git(worktree, &["merge-base", "--is-ancestor", base, head]).ok().map(|_| (*head).to_owned())
}

fn replay(options: &Options) -> Result<Replayed, String> {
    let checkout = super::repositories::checkout(&options.repo)?;
    let (worktree, _held) = free_worktree(&candidate::lander_worktree(&checkout, WORKTREE)?)?;
    candidate::ensure(&checkout, &worktree)?;
    git(&worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    let base = git(&worktree, &["rev-parse", &format!("{}^{{commit}}", options.base)])?;
    let read: Vec<(PathBuf, Ticket)> = options.tickets.iter().map(|path| queue::read(path).map(|ticket| (path.clone(), ticket)).map_err(|error| error.to_string())).collect::<Result<_, _>>()?;
    let candidate = landed_candidate(&worktree, &base, &read);
    let tickets: Vec<(PathBuf, Ticket)> = read.into_iter().map(|(path, ticket)| (path, Ticket { verdict: None, ..ticket })).collect();
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let built = match &candidate {
        Some(tip) => {
            git(&worktree, &["reset", "-q", "--hard"])?;
            git(&worktree, &["checkout", "-q", "-f", "--detach", tip])?;
            None
        }
        None => Some(candidate::rebuild_recorded(&worktree, &base, &tickets, scratch.path())?),
    };
    if let Some(built) = &built {
        for pick in &built.picks {
            match pick {
                Pick::Included => {}
                Pick::Decided(verdict) => return Ok(Replayed::of(verdict.split_whitespace().next().unwrap_or("undecided"))),
                Pick::Refused(refused) => return Ok(Replayed { verdict: "failed".to_owned(), failures: BTreeSet::from([Failure::Line(refused.verdict.split("; log:").next().unwrap_or_default().trim().to_owned())]) }),
                Pick::Undecided(reason) | Pick::Deferred(reason) => return Ok(Replayed::of(format!("undecided {reason}"))),
            }
        }
        if built.picks.len() != tickets.len() {
            return Ok(Replayed::of("undecided not every lane was picked"));
        }
    }
    let log = kept_log(&checkout, &base)?;
    let verdict = match checks::start(&checks::Launch { checker: &options.checker, offload_owner: "north-land-replay", round: "replay".to_owned(), red_fix: false, lander_slot: false, off_host: true, local: false, lanes: String::new(), tips: String::new() }, &worktree, &base, &log, false, None) {
        Ok(started) => started.wait(),
        Err(verdict) => verdict,
    };
    if verdict == Verdict::Green {
        let _ = fs::remove_file(&log);
    }
    let class = class(&verdict);
    let failures = match verdict {
        Verdict::Refused(_) | Verdict::RedMain(_) => named_failures(&log, &class),
        _ => BTreeSet::new(),
    };
    Ok(Replayed { verdict: class, failures })
}

pub fn free_worktree(first: &Path) -> Result<(PathBuf, Locked), String> {
    let numbered = |slot: usize| if slot == 1 { first.to_owned() } else { first.with_file_name(format!("{WORKTREE}-{slot}")) };
    for worktree in (1..=WORKTREES).map(numbered) {
        let lock = worktree.with_extension("lock");
        fs::create_dir_all(lock.parent().unwrap_or(Path::new("."))).map_err(|error| format!("{}: {error}", lock.display()))?;
        let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock).map_err(|error| format!("{}: {error}", lock.display()))?;
        if let Some(held) = hold::try_lock(file) {
            return Ok((worktree, held));
        }
    }
    Err(format!("all {WORKTREES} replay worktrees beside {} are in use by other replays", first.display()))
}

fn kept_log(checkout: &Path, base: &str) -> Result<PathBuf, String> {
    let origin = git(checkout, &["remote", "get-url", "--push", "origin"])?;
    let logs = queue::directory(&super::lander::state_home(), &origin).join("logs");
    fs::create_dir_all(&logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let micros = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_micros());
    Ok(logs.join(format!("{micros}-replay-{}.log", &base[..base.len().min(12)])))
}

fn named_failures(log: &Path, class: &str) -> BTreeSet<Failure> {
    let found = red_check::failures(&fs::read_to_string(log).unwrap_or_default());
    if !found.is_empty() {
        return found;
    }
    let check = class.split("; log:").next().unwrap_or(class).trim();
    BTreeSet::from([Failure::Line(check.to_owned())])
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match replay(&options) {
        Ok(replayed) => {
            for failure in &replayed.failures {
                println!("{FAILURE}{}", failure.key());
            }
            println!("{PREFIX}{}", replayed.verdict);
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

pub fn failures_of(output: &str) -> BTreeSet<Failure> {
    output.lines().filter_map(|line| line.strip_prefix(FAILURE)).filter_map(Failure::from_key).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_takes_the_next_worktree_while_another_replay_holds_the_first() {
        let container = tempfile::tempdir().unwrap();
        let first = container.path().join(WORKTREE);
        let (taken, held) = free_worktree(&first).unwrap();
        assert_eq!(taken, first);
        let (next, _next_held) = free_worktree(&first).unwrap();
        assert_eq!(next, container.path().join(format!("{WORKTREE}-2")));
        drop(held);
        assert_eq!(free_worktree(&first).unwrap().0, first, "a finished replay frees its worktree");
    }
}
