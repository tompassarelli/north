use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use super::candidate::{git, output};
use super::divergence::{self, Divergence};
use super::lander::state_home;
use super::{link_hold, queue, replay, repositories, telemetry};
use shadow::shadow;
use tolerance::{Held, only_fixes, tolerated};
use north_base::foundation::exit::NO_VERDICT;

mod arrival;
mod report;

pub use report::run;
use arrival::move_first;

const USAGE: &str = "usage: north land promote [--dry-run] [--shadow-minutes N] [--repo DIR]... [--checker PROGRAM]";
pub const REFUSED: &str = "lander-refused";
const ROUNDS: usize = 8;
const MATCHED_PAST_UNREACHED: usize = 2;
const WALK: usize = 256;
const SHADOW_MINUTES: u64 = 15;
const SHADOW_PAUSE: Duration = Duration::from_secs(30);
const LANDER_PATHS: [&str; 6] = ["crates/north-landing/", "crates/north-land-checks/", "crates/north-lander/", "crates/north-safe-push/", "crates/north-runtime/", "tests/it/land"];

mod shadow;
mod tolerance;

struct Options {
    dry_run: bool,
    shadow_minutes: u64,
    repos: Vec<PathBuf>,
    checker: String,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { dry_run: false, shadow_minutes: SHADOW_MINUTES, repos: Vec::new(), checker: "safe-push".to_owned() };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--dry-run" => options.dry_run = true,
            "--shadow-minutes" => options.shadow_minutes = rest.next()?.parse().ok()?,
            "--repo" => options.repos.push(PathBuf::from(rest.next()?)),
            "--checker" => options.checker = rest.next()?.clone(),
            _ => return None,
        }
    }
    if options.repos.is_empty() {
        options.repos.push(PathBuf::from("."));
    }
    Some(options)
}

struct Runtime {
    home: PathBuf,
}

impl Runtime {
    fn from_env() -> Self {
        Self { home: state_home().join("north") }
    }

    fn current(&self) -> PathBuf {
        self.home.join("current")
    }

    fn lander(&self) -> PathBuf {
        self.home.join("lander")
    }

    fn binary(&self, target: &Path) -> PathBuf {
        self.home.join(target).join(north_base::platform::harness::TREE).join("bin/north")
    }
}

pub fn swap(link: &Path, target: &Path) -> std::io::Result<()> {
    let parent = link.parent().ok_or_else(|| std::io::Error::other(format!("{} has no parent", link.display())))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".lander.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&temporary);
    std::os::unix::fs::symlink(target, &temporary)?;
    fs::rename(temporary, link)
}

struct Repository {
    checkout: PathBuf,
    queue: PathBuf,
}

impl Repository {
    fn open(repo: &Path) -> Result<Self, String> {
        let checkout = repositories::checkout(repo)?;
        let origin = git(&checkout, &["remote", "get-url", "--push", "origin"])?;
        git(&checkout, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
        Ok(Self { queue: queue::directory(&state_home(), &origin), checkout })
    }
}

struct Lane {
    path: PathBuf,
    branch: String,
    head: String,
}

struct Round {
    lanes: Vec<Lane>,
    verdict: String,
    decided: SystemTime,
    decided_on: Option<String>,
}

impl Round {
    fn tickets(&self) -> Vec<PathBuf> {
        self.lanes.iter().map(|lane| lane.path.clone()).collect()
    }

    fn names(&self) -> String {
        self.lanes.iter().map(|lane| lane.branch.as_str()).collect::<Vec<_>>().join(",")
    }
}

fn recorded(done: &Path) -> Vec<Round> {
    let Ok(entries) = fs::read_dir(done) else { return Vec::new() };
    let mut rounds: BTreeMap<String, Round> = BTreeMap::new();
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")) {
        let Ok(ticket) = queue::read(&path) else { continue };
        let Some(verdict) = ticket.verdict.filter(|verdict| replay::recorded_class(verdict).is_some()) else { continue };
        let decided = fs::metadata(&path).and_then(|metadata| metadata.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let key = match (verdict.starts_with("landed "), &ticket.decided_on) {
            (true, Some(base)) => format!("landed on {base}"),
            (true, None) => verdict.clone(),
            (false, _) => path.display().to_string(),
        };
        let round = rounds.entry(key).or_insert_with(|| Round { lanes: Vec::new(), verdict, decided, decided_on: ticket.decided_on.clone() });
        round.decided = round.decided.max(decided);
        round.lanes.push(Lane { path, branch: ticket.branch, head: ticket.head });
    }
    let mut rounds: Vec<Round> = rounds.into_values().collect();
    for round in &mut rounds {
        round.lanes.sort_by_key(|lane| (queue::queued_micros(&lane.path), lane.path.clone()));
    }
    rounds.sort_by_key(|round| round.decided);
    let skip = rounds.len().saturating_sub(ROUNDS);
    rounds.into_iter().skip(skip).collect()
}

fn count(checkout: &Path, base: &str, head: &str) -> Option<usize> {
    git(checkout, &["rev-list", "--count", &format!("{base}..{head}")]).ok()?.parse().ok()
}

fn base_of(checkout: &Path, round: &Round) -> Option<String> {
    if round.decided_on.is_some() {
        return round.decided_on.clone();
    }
    if let Some(landed) = round.verdict.strip_prefix("landed ") {
        return (0..=WALK).find_map(|depth| {
            let base = git(checkout, &["rev-parse", "-q", "--verify", &format!("{landed}~{depth}^{{commit}}")]).ok()?;
            let picked = round.lanes.iter().map(|lane| count(checkout, &base, &lane.head)).sum::<Option<usize>>()?;
            (picked == depth).then_some(base)
        });
    }
    round.decided_on.clone()
}

fn readable(checkout: &Path, round: &Round) -> bool {
    if let Some(tip) = replay::landed_tip(&round.verdict) {
        return output(checkout, &["cat-file", "-e", &format!("{tip}^{{commit}}")]).is_ok_and(|output| output.status.success());
    }
    round.lanes.iter().all(|lane| output(checkout, &["cat-file", "-e", &format!("{}^{{commit}}", lane.head)]).is_ok_and(|output| output.status.success()))
}

fn run_replay(binary: &Path, checkout: &Path, base: &str, checker: &str, tickets: &[PathBuf]) -> Result<replay::Replayed, String> {
    let ran = Command::new(binary)
        .args(["land", "replay", "--repo"])
        .arg(checkout)
        .args(["--base", base, "--checker", checker])
        .args(tickets)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{}: {error}", binary.display()))?;
    let stdout = String::from_utf8_lossy(&ran.stdout);
    match replay::verdict_of(&stdout) {
        Some(verdict) if ran.status.success() => Ok(replay::Replayed { verdict: verdict.to_owned(), failures: replay::failures_of(&stdout) }),
        _ => Err(format!("{} land replay exited {}: {}", binary.display(), ran.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&ran.stderr).trim().lines().last().unwrap_or(""))),
    }
}

fn replay_recorded(binary: &Path, held: &Held, repository: &Repository, checker: &str, divergences: &mut Vec<Divergence>, unreached: &mut Vec<String>) -> usize {
    let mut replayed = 0;
    for round in recorded(&repository.queue.join("done")) {
        let lanes = round.names();
        let class = replay::recorded_class(&round.verdict).unwrap_or_default();
        if let Some(signature) = replay::unfit_host(&round.verdict) {
            println!("north land promote: round {lanes} recorded {class} on an unfit check host ({signature}): not a verdict the candidate must reproduce");
            continue;
        }
        if !readable(&repository.checkout, &round) {
            println!("north land promote: round {lanes} recorded {class}: not replayable, a lane commit is gone");
            continue;
        }
        let Some(base) = base_of(&repository.checkout, &round) else {
            println!("north land promote: round {lanes} recorded {class}: not replayable, the lander did not record the main it was decided on");
            continue;
        };
        let outcome = run_replay(binary, &repository.checkout, &base, checker, &round.tickets()).unwrap_or_else(|error| replay::Replayed { verdict: format!("error {error}"), failures: Default::default() });
        let verdict = outcome.verdict.clone();
        let shown = verdict.split_whitespace().next().unwrap_or_default();
        if matches!(shown, "transient" | "deferred" | "error") && !replay::same(&round.verdict, &verdict) {
            println!("north land promote: round {lanes} on {} recorded {class} but the candidate's replay reached no verdict: {verdict}", &base[..base.len().min(12)]);
            unreached.push(format!("round {lanes}: {verdict}"));
            continue;
        }
        if replay::lands_what_was_refused(&round.verdict, &verdict) {
            println!("north land promote: round {lanes} on {} recorded {class} but the candidate lands it: an improvement, not a hold", &base[..base.len().min(12)]);
            replayed += 1;
            continue;
        }
        if !replay::same(&round.verdict, &verdict) {
            match tolerated(held, repository, checker, &round, &base, &outcome) {
                Ok(why) => {
                    println!("north land promote: round {lanes} on {} recorded {class} but the candidate replayed {shown}: not a hold, {why}", &base[..base.len().min(12)]);
                    replayed += 1;
                    continue;
                }
                Err(untolerated) => {
                    let held_note = if held.binary.is_some() { format!(" ({})", untolerated.why) } else { String::new() };
                    divergences.push(Divergence { checkout: repository.checkout.clone(), tickets: round.tickets(), lanes, base, recorded: round.verdict.clone(), replayed: format!("{verdict}{held_note}"), adds_failures: untolerated.adds_failures });
                    continue;
                }
            }
        }
        println!("north land promote: round {lanes} on {} recorded {class} replayed {shown}: matches", &base[..base.len().min(12)]);
        replayed += 1;
    }
    replayed
}

enum Outcome {
    Settled(String),
    Moved(String),
    MovedUnjudged { candidate: String, reason: String },
    Rolled { candidate: String, held: String, reason: String },
}

fn name(target: &Path) -> String {
    target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn promote(options: &Options) -> Result<Outcome, String> {
    let runtime = Runtime::from_env();
    let target = fs::read_link(runtime.current()).map_err(|error| format!("{}: {error}", runtime.current().display()))?;
    if let Some(settled) = already_settled(&runtime, &target) {
        return Ok(Outcome::Settled(settled));
    }
    let binary = runtime.binary(&target);
    if !binary.is_file() {
        return Err(format!("candidate {} has no {}", target.display(), binary.display()));
    }
    let repositories: Vec<Repository> = options.repos.iter().map(|repo| Repository::open(repo)).collect::<Result<_, _>>()?;
    let held_target = fs::read_link(runtime.lander()).ok();
    let held_binary = held_target.as_deref().map(|held| runtime.binary(held)).filter(|binary| binary.is_file());
    let fixes = held_target.as_deref().is_some_and(|held| repositories.iter().all(|repository| only_fixes(&repository.checkout, held, &target)));
    let held = Held { binary: held_binary.as_deref(), only_fixes: fixes };
    // Land first: the link moves as soon as current has its canary behind it, and the replay and shadow judge it after.
    if let Some(result) = move_first(options, &runtime, &repositories, &target)? {
        return Ok(Outcome::Settled(result));
    }
    println!("north land promote: candidate {} replays recorded rounds with {} after the link moved", target.display(), binary.display());
    let (mut divergences, mut unreached) = (Vec::new(), Vec::new());
    let mut replayed = 0;
    for repository in &repositories {
        replayed += replay_recorded(&binary, &held, repository, &options.checker, &mut divergences, &mut unreached);
    }
    if let Some(outcome) = unreached_only(replayed, divergences.is_empty(), &unreached, &target) {
        return Ok(outcome);
    }
    if !divergences.is_empty() {
        if let Some(outcome) = judge_diverged(options, &runtime, &repositories, held_target.as_deref(), &target, &divergences)? {
            return Ok(outcome);
        }
        replayed += divergences.len();
    }
    if replayed == 0 {
        return Ok(Outcome::MovedUnjudged { candidate: name(&target), reason: "no recorded round could be replayed".to_owned() });
    }
    let shadowed = match shadow(&binary, &repositories, &options.checker, options.shadow_minutes) {
        Ok(shadowed) => shadowed,
        Err(reason) => return roll_back(options, &runtime, held_target.as_deref(), &target, format!("the shadow lander refused it: {reason}")),
    };
    println!("north land promote: shadow: {shadowed}");
    let _ = fs::remove_file(runtime.home.join(REFUSED));
    let verb = if options.dry_run { "would point at" } else { "now points at" };
    Ok(Outcome::Moved(format!("the lander link {verb} {}; {replayed} recorded round(s) replayed and the shadow passed ({shadowed})", target.display())))
}

/// Rounds that reached no verdict and none that diverged: fewer than `MATCHED_PAST_UNREACHED` matches leave the link unjudged, more only report them.
fn unreached_only(replayed: usize, undiverged: bool, unreached: &[String], target: &Path) -> Option<Outcome> {
    if !undiverged || unreached.is_empty() {
        return None;
    }
    if replayed < MATCHED_PAST_UNREACHED {
        return Some(Outcome::MovedUnjudged { candidate: name(target), reason: unreached.join("; ") });
    }
    println!("north land promote: {} round(s) reached no verdict beside {replayed} that matched and none that diverged; reported, not held on: {}", unreached.len(), unreached.join("; "));
    None
}

fn already_settled(runtime: &Runtime, target: &Path) -> Option<String> {
    let unjudged = link_hold::unreached_of(&runtime.home).is_some_and(|unreached| unreached.candidate == name(target));
    if !unjudged && fs::read_link(runtime.lander()).ok().as_deref() == Some(target) {
        return Some(format!("the lander already runs {}", target.display()));
    }
    fs::read_to_string(runtime.home.join(REFUSED)).is_ok_and(|refused| Path::new(refused.trim()) == target).then(|| format!("the lander was rolled back from {}; it stays off the lander until current moves on", target.display()))
}

/// Judges the diverging rounds against the held runtime; `None` means the candidate follows, `Some` is the outcome that ends the promotion.
fn judge_diverged(options: &Options, runtime: &Runtime, repositories: &[Repository], held: Option<&Path>, target: &Path, divergences: &[Divergence]) -> Result<Option<Outcome>, String> {
    let replay = |binary: &Path, divergence: &Divergence| run_replay(binary, &divergence.checkout, &divergence.base, &options.checker, &divergence.tickets).map(|replayed| replayed.verdict);
    let sha = name(target);
    let source = repositories.iter().find(|repository| output(&repository.checkout, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok_and(|output| output.status.success())).unwrap_or(&repositories[0]);
    let runtimes = divergence::Runtimes { home: &runtime.home, source: &source.checkout, queue: &source.queue, held, candidate: target, dry_run: options.dry_run, replay: &replay };
    match divergence::judge(&runtimes, divergences) {
        Ok(divergence::Judged::Follows(judged)) => {
            println!("north land promote: {judged}");
            Ok(None)
        }
        Ok(divergence::Judged::Retry(reason)) => Ok(Some(Outcome::MovedUnjudged { candidate: sha, reason })),
        Ok(divergence::Judged::Refused(reason)) | Err(reason) => roll_back(options, runtime, held, target, reason).map(Some),
    }
}

/// Moves the link back to the runtime it held before this promotion, so a runtime whose own replay diverged from the recorded verdict no longer runs the lander, and names it refused until current moves on, as the doctor's rollback does.
fn roll_back(options: &Options, runtime: &Runtime, held: Option<&Path>, candidate: &Path, reason: String) -> Result<Outcome, String> {
    if options.dry_run {
        let held = held.map_or_else(|| "no held runtime".to_owned(), |held| held.display().to_string());
        return Ok(Outcome::Rolled { candidate: name(candidate), held, reason });
    }
    let Some(held) = held else {
        return Ok(Outcome::MovedUnjudged { candidate: name(candidate), reason: format!("{reason}; no held runtime to move the link back to") });
    };
    let refused = runtime.home.join(REFUSED);
    fs::write(&refused, format!("{}\n", candidate.display())).map_err(|error| format!("{}: {error}", refused.display()))?;
    if fs::read_link(runtime.lander()).ok().as_deref() == Some(candidate) {
        swap(&runtime.lander(), held).map_err(|error| format!("{}: {error}", runtime.lander().display()))?;
    }
    println!("north land promote: the lander link is back at {}", held.display());
    Ok(Outcome::Rolled { candidate: name(candidate), held: held.display().to_string(), reason })
}

