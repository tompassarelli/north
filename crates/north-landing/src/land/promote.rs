use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use super::candidate::{git, output};
use super::lander::state_home;
use super::{queue, replay, repositories};

const USAGE: &str = "usage: north land promote [--dry-run] [--shadow-minutes N] [--repo DIR]... [--checker PROGRAM]";
const ROUNDS: usize = 8;
const WALK: usize = 256;
const SHADOW_MINUTES: u64 = 15;
const SHADOW_PAUSE: Duration = Duration::from_secs(30);
const UNITS: &str = "north-land*";

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

fn swap(link: &Path, target: &Path) -> std::io::Result<()> {
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
        let key = if verdict.starts_with("landed ") { verdict.clone() } else { path.display().to_string() };
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
    round.lanes.iter().all(|lane| output(checkout, &["cat-file", "-e", &format!("{}^{{commit}}", lane.head)]).is_ok_and(|output| output.status.success()))
}

fn run_replay(binary: &Path, checkout: &Path, base: &str, checker: &str, tickets: &[PathBuf]) -> Result<String, String> {
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
        Some(verdict) if ran.status.success() => Ok(verdict.to_owned()),
        _ => Err(format!("{} land replay exited {}: {}", binary.display(), ran.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&ran.stderr).trim().lines().last().unwrap_or(""))),
    }
}

fn replay_recorded(binary: &Path, repository: &Repository, checker: &str) -> Result<usize, String> {
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
        let verdict = run_replay(binary, &repository.checkout, &base, checker, &round.tickets()).unwrap_or_else(|error| format!("error {error}"));
        let shown = verdict.split_whitespace().next().unwrap_or_default();
        if !replay::same(&round.verdict, &verdict) {
            return Err(format!("round {lanes} on {base} recorded {class} but the candidate replayed {verdict}"));
        }
        println!("north land promote: round {lanes} on {} recorded {class} replayed {shown}: matches", &base[..base.len().min(12)]);
        replayed += 1;
    }
    Ok(replayed)
}

fn shadow_copy(queue: &Path, scratch: &Path) -> Result<Vec<PathBuf>, String> {
    let pending = queue::pending(queue).map_err(|error| format!("{}: {error}", queue.display()))?;
    pending
        .into_iter()
        .map(|(path, ticket)| {
            let copy = scratch.join(path.file_name().unwrap_or_default());
            queue::write(&copy, &ticket).map(|()| copy).map_err(|error| error.to_string())
        })
        .collect()
}

fn shadow(binary: &Path, repositories: &[Repository], checker: &str, minutes: u64) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(minutes * 60);
    let mut errors: HashSet<String> = HashSet::new();
    let (mut passes, mut checked) = (0, 0);
    loop {
        passes += 1;
        for repository in repositories {
            let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
            let tickets = shadow_copy(&repository.queue, scratch.path())?;
            if tickets.is_empty() {
                continue;
            }
            checked += 1;
            let error = match run_replay(binary, &repository.checkout, "FETCH_HEAD", checker, &tickets) {
                Ok(verdict) => {
                    println!("north land promote: shadow pass {passes}: {} queued lane(s) of {} replayed {verdict}", tickets.len(), repository.checkout.display());
                    verdict.starts_with("transient").then_some(verdict)
                }
                Err(error) => Some(error),
            };
            if let Some(error) = error {
                println!("north land promote: shadow pass {passes}: error {error}");
                if !errors.insert(error.clone()) {
                    return Err(format!("the shadow lander repeated an error: {error}"));
                }
            }
        }
        if Instant::now() + SHADOW_PAUSE >= deadline {
            break;
        }
        std::thread::sleep(SHADOW_PAUSE);
    }
    Ok(format!("{passes} pass(es), {checked} candidate(s) checked, {} error(s), none repeated", errors.len()))
}

fn restart() -> Result<(), String> {
    let status = Command::new("systemctl").args(["--user", "restart", UNITS]).stdin(Stdio::null()).status().map_err(|error| format!("systemctl: {error}"))?;
    if status.success() { Ok(()) } else { Err(format!("systemctl --user restart {UNITS} exited {}", status.code().map_or("on a signal".to_owned(), |code| code.to_string()))) }
}

enum Outcome {
    Settled(String),
    Swap { link: PathBuf, target: PathBuf },
}

fn promote(options: &Options) -> Result<Outcome, String> {
    let runtime = Runtime::from_env();
    let target = fs::read_link(runtime.current()).map_err(|error| format!("{}: {error}", runtime.current().display()))?;
    if fs::read_link(runtime.lander()).ok().as_deref() == Some(target.as_path()) {
        return Ok(Outcome::Settled(format!("the lander already runs {}", target.display())));
    }
    let binary = runtime.binary(&target);
    if !binary.is_file() {
        return Err(format!("candidate {} has no {}", target.display(), binary.display()));
    }
    println!("north land promote: candidate {} replays recorded rounds with {}", target.display(), binary.display());
    let repositories: Vec<Repository> = options.repos.iter().map(|repo| Repository::open(repo)).collect::<Result<_, _>>()?;
    let mut replayed = 0;
    for repository in &repositories {
        replayed += replay_recorded(&binary, repository, &options.checker)?;
    }
    if replayed == 0 {
        return Err("no recorded round could be replayed".to_owned());
    }
    let shadowed = shadow(&binary, &repositories, &options.checker, options.shadow_minutes)?;
    println!("north land promote: shadow: {shadowed}");
    if options.dry_run {
        return Ok(Outcome::Settled(format!("dry run: {replayed} round(s) matched; {} would point at {}", runtime.lander().display(), target.display())));
    }
    Ok(Outcome::Swap { link: runtime.lander(), target })
}

fn settle(link: &Path, target: &Path) -> u8 {
    if let Err(error) = swap(link, target) {
        println!("north land promote: refused: {}: {error}; the lander link is unchanged", link.display());
        return 1;
    }
    match restart() {
        Ok(()) => {
            println!("north land promote: {} now points at {}; restarted {UNITS}", link.display(), target.display());
            0
        }
        Err(error) => {
            println!("north land promote: {} now points at {}, but {error}", link.display(), target.display());
            1
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match promote(&options) {
        Ok(Outcome::Swap { link, target }) => settle(&link, &target),
        Ok(Outcome::Settled(result)) => {
            println!("north land promote: {result}");
            0
        }
        Err(error) => {
            println!("north land promote: refused: {error}; the lander link is unchanged");
            1
        }
    }
}
