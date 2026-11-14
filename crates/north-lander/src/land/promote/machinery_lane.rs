//! `north land machinery-replay BASE..HEAD`: a lane that changes a `landing_path_paths` file is judged under its own machinery before it lands. The last recorded rounds of each queue are replayed through the lane's own build, and each that the lane's build replays unlike the recorded verdict is replayed again under the machinery the lander runs now, so the lane is refused only for a verdict its own change moves (north#1537). It reads the lander link, the recorded tickets and the replay worktrees, so it runs outside the candidate confinement, from a trusted caller.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Repository, Runtime, base_of, readable, recorded, run_replay};
use crate::land::candidate::git;
use crate::land::replay::{self, Replayed};
use crate::landing::red_check::Failure;
use north_base::foundation::exit::NO_VERDICT;
use north_base::platform::land_queue::ROUND;
use north_base::platform::trust_boundary;

const USAGE: &str = "usage: north land machinery-replay BASE..HEAD [--repo DIR]...";
const ROUNDS: usize = 3;
const DECLARATION: &str = "Replay-flip:";
const FLIPS: &str = "docs/replay-flips.md";
const NESTED: &str = "replay";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Landed,
    Refused,
}

fn side(verdict: &str) -> Option<Side> {
    match verdict.split_whitespace().next()? {
        "landed" => Some(Side::Landed),
        "failed" | "conflict" | "red-main" => Some(Side::Refused),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Judged {
    Matches,
    Shared,
    Declared,
    Unreached(String),
    Refused(String),
}

/// How the lane's replay `lane` of the round `names`, recorded as `recorded`, judges the lane; `main` replays the round under the machinery the lander runs now and is asked only when the lane's replay differs from the record.
pub(super) fn judge(names: &str, recorded: &str, lane: &Replayed, main: impl FnOnce() -> Replayed, declared: &BTreeSet<String>) -> Judged {
    let was = side(recorded);
    let now = side(&lane.verdict);
    if was.is_none() || now == was {
        return Judged::Matches;
    }
    let main = main();
    match (now, side(&main.verdict)) {
        (None, None) => Judged::Unreached(format!("round {names} reaches no verdict under main's machinery ({}) nor under the lane's ({})", main.verdict, lane.verdict)),
        (None, Some(_)) => Judged::Refused(format!("round {names} replays {} under main's machinery and reaches no verdict under the lane's: {}", main.verdict, lane.verdict)),
        (Some(_), None) if declared.contains(names) => Judged::Declared,
        (Some(_), None) => Judged::Unreached(format!("round {names} replays {} under the lane's build, unlike its record, and main's machinery reaches no verdict to compare ({})", lane.verdict, main.verdict)),
        (Some(now), Some(before)) => moved(names, (lane, now), (&main, before), declared),
    }
}

fn moved(names: &str, (lane, now): (&Replayed, Side), (main, before): (&Replayed, Side), declared: &BTreeSet<String>) -> Judged {
    let cases: Vec<&str> = lane.failures.difference(&main.failures).filter(|failure| matches!(failure, Failure::Cargo(_) | Failure::Harness(_))).map(Failure::label).collect();
    if !cases.is_empty() {
        Judged::Refused(format!("round {names}: {} pass under main's machinery and fail under the lane's", cases.join(", ")))
    } else if now == before {
        Judged::Shared
    } else if declared.contains(names) {
        Judged::Declared
    } else {
        Judged::Refused(format!("round {names} replays {} under main's machinery and {} under the lane's; a lane that moves it on purpose adds a `{DECLARATION} {names} :: WHY` line to {FLIPS}", main.verdict, lane.verdict))
    }
}

/// The rounds the lane declares it moves on purpose: each `Replay-flip: ROUND :: WHY` line with a reason that its diff `added` adds to docs/replay-flips.md, so the declaration is part of the patch its reviewers accept.
pub(super) fn declared(added: &str) -> BTreeSet<String> {
    added
        .lines()
        .filter_map(|line| line.strip_prefix('+').filter(|_| !line.starts_with("+++")))
        .filter_map(|line| line.trim().trim_start_matches("- ").strip_prefix(DECLARATION))
        .filter_map(|rest| rest.split_once("::"))
        .filter(|(_, why)| !why.trim().is_empty())
        .map(|(round, _)| round.trim().to_owned())
        .collect()
}

struct Machinery {
    binary: PathBuf,
    checker: String,
}

impl Machinery {
    fn linked(binary: PathBuf, scratch: &Path, name: &str) -> Result<Self, String> {
        let directory = scratch.join(name);
        fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        let checker = directory.join("safe-push");
        std::os::unix::fs::symlink(&binary, &checker).map_err(|error| format!("{}: {error}", checker.display()))?;
        Ok(Self { binary, checker: checker.display().to_string() })
    }

    fn replay(&self, repository: &Repository, base: &str, tickets: &[PathBuf]) -> Replayed {
        run_replay(&self.binary, &repository.checkout, base, &self.checker, tickets).unwrap_or_else(|error| Replayed { verdict: format!("error {error}"), failures: BTreeSet::new() })
    }
}

fn canonical(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

/// The binaries of the runtimes this host runs live, which are main's machinery and no build of a lane.
fn live_binaries(runtime: &Runtime) -> Vec<PathBuf> {
    [runtime.current(), runtime.lander()].iter().flat_map(|link| [link.join("north"), link.join(north_base::platform::harness::TREE).join("bin/north")]).filter_map(|path| canonical(&path)).collect()
}

#[derive(Default)]
struct Tally {
    judged: usize,
    unreached: Vec<String>,
    refused: Vec<String>,
}

fn replay_repository(lane: &Machinery, main: &Machinery, repository: &Repository, declared: &BTreeSet<String>, tally: &mut Tally) {
    for round in recorded(&repository.queue.join("done"), ROUNDS) {
        let names = round.names();
        if let Some(signature) = replay::unfit_host(&round.verdict) {
            println!("north land machinery-replay: round {names} was decided on an unfit check host ({signature}): not replayed");
            continue;
        }
        let Some(base) = readable(&repository.checkout, &round).then(|| base_of(&repository.checkout, &round)).flatten() else {
            println!("north land machinery-replay: round {names}: not replayable, a lane commit or its base is gone");
            continue;
        };
        let tickets = round.tickets();
        let replayed = lane.replay(repository, &base, &tickets);
        let judged = judge(&names, &round.verdict, &replayed, || main.replay(repository, &base, &tickets), declared);
        let class = replay::recorded_class(&round.verdict).unwrap_or_default();
        println!("north land machinery-replay: round {names} on {} recorded {class}, the lane's build replays {}: {judged:?}", &base[..base.len().min(12)], replayed.verdict);
        match judged {
            Judged::Unreached(why) => tally.unreached.push(why),
            Judged::Refused(why) => tally.refused.push(why),
            Judged::Matches | Judged::Shared | Judged::Declared => tally.judged += 1,
        }
    }
}

fn changed(range: &str) -> Result<Vec<String>, String> {
    let (base, head) = range.split_once("..").filter(|(base, head)| !base.is_empty() && !head.is_empty()).ok_or_else(|| USAGE.to_owned())?;
    let here = Path::new(".");
    let fork = git(here, &["merge-base", base, head])?;
    Ok(git(here, &["diff", "--name-only", "--no-renames", &fork, head])?.lines().map(str::to_owned).collect())
}

fn machinery_replay(range: &str, repos: &[PathBuf]) -> Result<u8, String> {
    if std::env::var(ROUND).is_ok_and(|round| round == NESTED) {
        println!("north land machinery-replay: this check runs inside a replayed round, which replays nothing again");
        return Ok(0);
    }
    let files = changed(range)?;
    let touched = trust_boundary::listed(trust_boundary::LANDING_PATH).map(|list| trust_boundary::crossing_in(&list, &files)).unwrap_or_default();
    if touched.is_empty() {
        println!("north land machinery-replay: the lane changes no landing_path_paths file, so nothing is replayed");
        return Ok(0);
    }
    let runtime = Runtime::from_env();
    let exe = std::env::current_exe().map_err(|error| format!("the running north: {error}"))?;
    let own = canonical(&exe).unwrap_or(exe);
    if live_binaries(&runtime).contains(&own) {
        println!("north land machinery-replay: the lane changes {} and runs as the live runtime {}, so nothing is replayed: those files compile into no build of the lane's own", touched.join(", "), own.display());
        return Ok(0);
    }
    let lander = fs::read_link(runtime.lander()).map_err(|error| format!("{}: {error}", runtime.lander().display()))?;
    let main_binary = runtime.binary(&lander);
    if !main_binary.is_file() {
        return Err(format!("the lander's runtime {} has no {}", lander.display(), main_binary.display()));
    }
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let lane = Machinery::linked(own, scratch.path(), "lane")?;
    let main = Machinery::linked(main_binary, scratch.path(), "main")?;
    let (base, head) = range.split_once("..").unwrap_or_default();
    let fork = git(Path::new("."), &["merge-base", base, head])?;
    let declared = declared(&git(Path::new("."), &["diff", "-U0", &fork, head, "--", FLIPS])?);
    println!("north land machinery-replay: the lane changes {}; replaying the last {ROUNDS} recorded rounds of each queue through {}", touched.join(", "), lane.binary.display());
    let mut tally = Tally::default();
    for repo in repos {
        let repository = Repository::open(repo)?;
        replay_repository(&lane, &main, &repository, &declared, &mut tally);
    }
    for why in &tally.refused {
        println!("north land machinery-replay: refused: {why}");
    }
    if !tally.refused.is_empty() {
        return Ok(1);
    }
    if !tally.unreached.is_empty() {
        println!("north land machinery-replay: no verdict: {}", tally.unreached.join("; "));
        return Ok(NO_VERDICT);
    }
    println!("north land machinery-replay: {} round(s) judged, none moved by the lane", tally.judged);
    Ok(0)
}

pub fn run(arguments: &[String]) -> u8 {
    let mut rest = arguments.iter();
    let (mut range, mut repos) = (None, Vec::new());
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--repo" if let Some(repo) = rest.next() => repos.push(PathBuf::from(repo)),
            given if range.is_none() && !given.starts_with('-') => range = Some(given.to_owned()),
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    let Some(range) = range else {
        eprintln!("{USAGE}");
        return 64;
    };
    if repos.is_empty() {
        repos.push(PathBuf::from("."));
    }
    machinery_replay(&range, &repos).unwrap_or_else(|error| {
        eprintln!("north land machinery-replay: {error}");
        NO_VERDICT
    })
}

#[cfg(test)]
#[path = "machinery_lane_tests.rs"]
mod tests;
