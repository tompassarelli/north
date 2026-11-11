use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::land_ticket::{self, Phase};
use north_base::lifecycle::Store;
use north_check_environment::{Environment, instance};
use north_base::platform::land_queue;
use north_base::platform::ssh_retry::{Link, Reply, Rerun, retried};
use north_lander::land::after::follow;
use north_lander::land::candidate::git;
use north_lander::land::{link_hold, pause, post_land, queue, watchdog};

mod probe;

pub use probe::{Probe, REMOVE_QUEUE, orphan_scan, parse_probe, probe};

pub const INFRASTRUCTURE_WINDOW_SECS: u64 = 2 * 3600;
pub const DECIDED_WINDOW_SECS: u64 = 24 * 3600;
pub const PROBE_SECONDS: &str = "60";
pub const LANDING_GAP_SECS: u64 = 30;
pub const INFRASTRUCTURE: &[&str] = &["is a missing but already registered worktree", "is not on this host", "lacks the commits under", "No space left on device"];
pub const TOOLCHAINS: &[&str] = &["gcc", "binutils", "glibc", "clang", "llvm", "rustc", "ghc"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub reach: Reach,
}

/// How the doctor runs a script on a place: here, over ssh, or here under a scratch environment (and inside the user and mount namespace
/// the process `namespace` holds, when one is named), which is how `north land drill` gives a scratch lander its own home and disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reach {
    Here,
    Ssh(String),
    Scratch { env: Vec<(String, String)>, namespace: Option<u32> },
}

#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub queues: Vec<QueueFacts>,
    pub link: Option<LinkFacts>,
    pub places: Vec<PlaceFacts>,
    pub unserved: Vec<String>,
    pub orphaned: Vec<PathBuf>,
    pub unreachable: Vec<String>,
    pub reach_error: Option<String>,
    /// The stray queues the probe found on the offload hosts, with the host each sits on.
    pub orphaned_remote: Vec<(Place, PathBuf)>,
}

#[derive(Clone, Debug, Default)]
pub struct QueueFacts {
    pub name: String,
    pub queue: PathBuf,
    pub unit: String,
    pub oldest_queued: Option<u64>,
    pub since_landing: Option<u64>,
    pub since_decision: Option<u64>,
    pub round_silence: Option<u64>,
    pub paused: Option<String>,
    pub broken_for: Option<u64>,
    pub environment: Option<Environment>,
    pub unit_state: Option<String>,
    pub infrastructure: Vec<Infrastructure>,
    pub source_builds: Vec<SourceBuild>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBuild {
    pub log: PathBuf,
    pub derivation: String,
    pub offline: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Infrastructure {
    pub ticket: PathBuf,
    pub branch: String,
    pub signature: String,
}

#[derive(Clone, Debug, Default)]
pub struct LinkFacts {
    pub home: PathBuf,
    pub lander: String,
    pub current: String,
    pub behind: Option<Behind>,
    pub lag: Option<u64>,
    pub green_since_link: bool,
    pub refused: Option<String>,
    pub builds: BTreeSet<String>,
    pub units: Vec<String>,
    pub unreached: Option<link_hold::Unreached>,
}

#[derive(Clone, Debug, Default)]
pub struct Behind {
    pub landings: Option<usize>,
    pub secs: u64,
    pub reason: String,
}

impl std::fmt::Display for Behind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.landings {
            Some(landings) => write!(formatter, "{landings} landing(s) and {} min", self.secs / 60),
            None => write!(formatter, "{} min", self.secs / 60),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlaceFacts {
    pub place: Place,
    pub probe: Result<Probe, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotHold {
    pub slot: String,
    pub held: u64,
    pub pgid: Option<u32>,
    pub locked: bool,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn age(path: &Path, now: u64) -> Option<u64> {
    let modified = fs::metadata(path).and_then(|metadata| metadata.modified()).ok()?;
    Some(now.saturating_sub(modified.duration_since(UNIX_EPOCH).ok()?.as_secs()))
}

fn link_age(path: &Path, now: u64) -> Option<u64> {
    let modified = fs::symlink_metadata(path).and_then(|metadata| metadata.modified()).ok()?;
    Some(now.saturating_sub(modified.duration_since(UNIX_EPOCH).ok()?.as_secs()))
}

pub fn queue_facts(name: &str, queue: &Path, repo: &Path, now: u64) -> QueueFacts {
    served_queue_facts(name, queue, land_queue::serving_unit(repo).unwrap_or_else(|| watchdog::unit(repo)), now)
}

/// The facts of the queue at `queue`, which `unit` serves.
pub fn served_queue_facts(name: &str, queue: &Path, unit: String, now: u64) -> QueueFacts {
    let pending = queue::pending(queue).unwrap_or_default();
    let oldest_queued = pending.iter().map(|(path, _)| u64::try_from(queue::queued_micros(path) / 1_000_000).unwrap_or(u64::MAX)).min().map(|queued| now.saturating_sub(queued));
    let decided = decided(queue, now);
    let since_landing = decided.iter().filter(|decision| decision.phase == Phase::Landed).map(|decision| decision.age).min();
    let since_decision = decided.iter().map(|decision| decision.age).min();
    let infrastructure = decided
        .iter()
        .filter(|decision| decision.age <= INFRASTRUCTURE_WINDOW_SECS && decision.phase == Phase::Failed)
        .filter(|decision| !pending.iter().any(|(_, ticket)| ticket.branch == decision.branch) && !reran(queue, &decision.path))
        .filter_map(|decision| infrastructure_of(&decision.path, &decision.branch, &decision.verdict))
        .collect();
    QueueFacts {
        name: name.to_owned(),
        queue: queue.to_owned(),
        unit_state: unit_state(&unit),
        unit,
        oldest_queued,
        since_landing,
        since_decision,
        round_silence: silence(queue, now),
        paused: pause::held(queue),
        broken_for: broken_for(queue, now),
        environment: Some(north_check_environment::active(queue)),
        infrastructure,
        source_builds: source_builds(&queue.join("logs"), now),
    }
}

/// Seconds since main's check environment first broke, not since the pause's last renewal; `None` while it passes.
fn broken_for(queue: &Path, now: u64) -> Option<u64> {
    let moves = Store::host().ok()?.history("base-broken", &instance(queue)).ok()?;
    let moves: Vec<_> = moves.iter().filter(|logged| !logged.refused).collect();
    let began = moves.iter().rposition(|logged| logged.to == "clear").map_or(0, |cleared| cleared + 1);
    let first = moves.get(began).filter(|_| moves.last().is_some_and(|last| last.to != "clear"))?;
    Some(now.saturating_sub(u64::try_from(first.at.unix_seconds()).unwrap_or(now)))
}

struct Decided {
    age: u64,
    verdict: String,
    phase: Phase,
    branch: String,
    path: PathBuf,
}

fn decided(queue: &Path, now: u64) -> Vec<Decided> {
    let Ok(entries) = fs::read_dir(queue.join("done")) else { return Vec::new() };
    let store = land_ticket::recorded(queue);
    let mut decided: Vec<Decided> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .filter_map(|path| Some((age(&path, now)?, path)))
        .filter(|(age, _)| *age <= DECIDED_WINDOW_SECS)
        .filter_map(|(age, path)| {
            let ticket = queue::read(&path).ok()?;
            let phase = land_ticket::read(store.as_ref(), queue, &path, ticket.verdict.as_deref());
            Some(Decided { age, verdict: ticket.verdict?, phase, branch: ticket.branch, path })
        })
        .collect();
    decided.sort_by_key(|decision| decision.age);
    let mut newest = BTreeSet::new();
    decided.retain(|decision| newest.insert(decision.branch.clone()));
    decided
}

pub fn reran(queue: &Path, ticket: &Path) -> bool {
    land_ticket::recorded(queue).is_some_and(|store| land_ticket::requeued_after_failure(&store, queue, ticket))
}

/// The infrastructure signature in the logs a refused ticket's verdict names, when one is there.
pub fn infrastructure_of(ticket: &Path, branch: &str, verdict: &str) -> Option<Infrastructure> {
    let logs = verdict.split("log: ").skip(1).filter_map(|rest| rest.split([';', ' ', '\n']).next()).map(PathBuf::from);
    let signature = std::iter::once(verdict.to_owned()).chain(logs.filter_map(|log| fs::read_to_string(log).ok())).find_map(|text| INFRASTRUCTURE.iter().find(|signature| text.contains(**signature)).map(|signature| (*signature).to_owned()))?;
    Some(Infrastructure { ticket: ticket.to_owned(), branch: branch.to_owned(), signature })
}

/// The toolchain derivation a land check log last started building from source, as nix prints `building '/nix/store/HASH-NAME.drv'`.
pub fn toolchain_build(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let path = line.strip_prefix("building '")?.split('\'').next()?;
        let name = path.strip_prefix("/nix/store/")?.split_once('-')?.1.strip_suffix(".drv")?;
        let (stem, version) = TOOLCHAINS.iter().find_map(|stem| Some((*stem, name.strip_prefix(stem)?.strip_prefix('-')?)))?;
        version.starts_with(|first: char| first.is_ascii_digit()).then(|| format!("{stem}-{version}"))
    })
}

/// Whether a check log may still be written: younger than a check slot's deadline and no `.outcome` beside it written after it.
fn running(log: &Path, now: u64) -> bool {
    let Some(written) = age(log, now) else { return false };
    written <= crate::bounds::SLOT_DEADLINE_SECS && age(&log.with_extension("outcome"), now).is_none_or(|finished| finished > written)
}

pub fn source_builds(logs: &Path, now: u64) -> Vec<SourceBuild> {
    let Ok(entries) = fs::read_dir(logs) else { return Vec::new() };
    let mut found: Vec<SourceBuild> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "log") && running(path, now))
        .filter_map(|log| {
            let text = fs::read_to_string(&log).ok()?;
            Some(SourceBuild { derivation: toolchain_build(&text)?, offline: text.lines().any(|line| line.starts_with("safe-push: land check: ") && line.contains(" --offline")), log })
        })
        .collect();
    found.sort_by(|left, right| left.log.cmp(&right.log));
    found
}

pub const MISSING_UNIT: &str = "not-found";

/// The unit's ActiveState, or `not-found` when systemd loads no such unit.
fn unit_state(unit: &str) -> Option<String> {
    let output = Command::new("systemctl").args(["--user", "show", "-p", "LoadState,ActiveState", unit]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let field = |name: &str| text.lines().find_map(|line| line.strip_prefix(name)).map(str::to_owned);
    if field("LoadState=").as_deref() == Some(MISSING_UNIT) { Some(MISSING_UNIT.to_owned()) } else { field("ActiveState=").filter(|state| !state.is_empty()) }
}

fn silence(queue: &Path, now: u64) -> Option<u64> {
    let logs = fs::read_dir(queue.join("logs")).into_iter().flatten().filter_map(Result::ok).filter_map(|entry| age(&entry.path(), now));
    std::iter::once(queue.join(watchdog::ROUNDS)).filter_map(|path| age(&path, now)).chain(logs).min()
}

fn target(link: &Path) -> Option<String> {
    fs::read_link(link).ok().map(|target| target.to_string_lossy().into_owned())
}

pub fn link_facts(home: &Path, north: Option<&Path>, units: Vec<String>, now: u64) -> Option<LinkFacts> {
    let lander = target(&home.join("lander"))?;
    let current = target(&home.join("current")).unwrap_or_default();
    let behind = (lander != current).then(|| behind(home, north, &lander, &current, now));
    let lag = north.and_then(|checkout| lag(checkout, &lander));
    let link_set = link_age(&home.join("lander"), now).unwrap_or(0);
    let builds = fs::read_dir(home.join("runtimes"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(north_base::platform::harness::TREE).join("bin/north").is_file())
        .map(|entry| format!("runtimes/{}", entry.file_name().to_string_lossy()))
        .collect();
    let refused = fs::read_to_string(home.join(crate::REFUSED)).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty());
    Some(LinkFacts { home: home.to_owned(), green_since_link: green_since(&post_land::ledger_path(), now.saturating_sub(link_set)), lander, current, behind, lag, refused, builds, units, unreached: link_hold::unreached_of(home) })
}

fn behind(home: &Path, north: Option<&Path>, lander: &str, current: &str, now: u64) -> Behind {
    let unreached = link_hold::unreached_of(home).filter(|unreached| current.ends_with(&unreached.candidate)).map(|unreached| format!("{} promotion(s) in a row reached no verdict: {}", unreached.attempts, unreached.reason));
    let reason = unreached.or_else(|| link_hold::reason(home)).unwrap_or_else(|| "no hold recorded: north land promote has not judged current yet".to_owned());
    let sha = |link: &str| Path::new(link).file_name().map(|name| name.to_string_lossy().into_owned());
    let range = sha(lander).zip(sha(current)).map(|(lander, current)| format!("{lander}..{current}"));
    let landed: Option<Vec<u64>> = north.zip(range).and_then(|(checkout, range)| git(checkout, &["log", "--first-parent", "--format=%ct", &range]).ok()).map(|times| times.lines().filter_map(|time| time.trim().parse().ok()).collect());
    match landed.filter(|times| !times.is_empty()) {
        Some(times) => Behind { landings: Some(landings(&times)), secs: now.saturating_sub(times.iter().copied().min().unwrap_or(now)), reason },
        None => Behind { landings: None, secs: link_hold::since(home).map_or_else(|| link_age(&home.join("current"), now).unwrap_or(0), |since| now.saturating_sub(since)), reason },
    }
}

pub fn landings(committed: &[u64]) -> usize {
    let mut times = committed.to_vec();
    times.sort_unstable();
    1 + times.windows(2).filter(|pair| pair[1] - pair[0] > LANDING_GAP_SECS).count()
}

fn lag(checkout: &Path, lander: &str) -> Option<u64> {
    let sha = Path::new(lander).file_name()?.to_str()?;
    let at = |revision: &str| git(checkout, &["log", "-1", "--format=%ct", revision]).ok()?.trim().parse::<u64>().ok();
    let main = at("origin/main")?;
    let ours = at(sha)?;
    Some(main.saturating_sub(ours))
}

/// Whether the after-landing ledger holds a green run that started after `since` (epoch seconds).
pub fn green_since(ledger: &Path, since: u64) -> bool {
    fs::read_to_string(ledger).unwrap_or_default().lines().any(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        let finished = fields.first().and_then(|value| value.parse::<u64>().ok());
        let seconds = fields.get(6).and_then(|value| value.parse::<u64>().ok()).unwrap_or(0);
        fields.get(3) == Some(&"green") && finished.is_some_and(|finished| finished.saturating_sub(seconds) > since)
    })
}

/// Runs `script` with `arguments` under bash on `place`: here, or over ssh with the script on stdin.
pub fn shell(place: &Place, script: &str, arguments: &[&str]) -> Result<String, String> {
    match &place.reach {
        Reach::Ssh(ssh) => retried(ssh, Rerun::Anywhere, |link| attempt(place, script, arguments, link)),
        _ => attempt(place, script, arguments, Link::Shared).value,
    }
}

fn attempt(place: &Place, script: &str, arguments: &[&str], link: Link) -> Reply<Result<String, String>> {
    let mut command = Command::new("timeout");
    command.arg(PROBE_SECONDS);
    match &place.reach {
        Reach::Ssh(ssh) => command.args(["ssh", "-T"]).args(link.options()).args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", "bash", "-s", "--"]),
        Reach::Here => command.args(["bash", "-s", "--"]),
        Reach::Scratch { env, namespace } => {
            if let Some(pid) = namespace {
                command.args(["nsenter", "--preserve-credentials", "-U", "-m", "-t", &pid.to_string()]);
            }
            command.env_remove("NORTH_OFFLOAD_ROOT").envs(env.iter().map(|(key, value)| (key, value))).args(["bash", "-s", "--"])
        }
    };
    let output = piped(command.args(arguments), script).map_err(|error| format!("{}: {error}", place.name));
    match output {
        Err(error) => Reply { value: Err(error), exit: None, said: String::new() },
        Ok(output) => {
            let said = String::from_utf8_lossy(&output.stderr).into_owned();
            let value = if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(format!("{} exited {}: {}", place.name, output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), said.trim())) };
            Reply { value, exit: output.status.code(), said }
        }
    }
}

fn piped(command: &mut Command, script: &str) -> std::io::Result<std::process::Output> {
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes())?;
    }
    child.wait_with_output()
}

pub fn place_facts(place: Place) -> PlaceFacts {
    let probe = shell(&place, &probe(), &[]).and_then(|text| parse_probe(&text));
    PlaceFacts { place, probe }
}

/// The stray queues each remote place's probe named, with the place each sits on; the local place's own queue is `discovery.orphaned` instead.
pub fn remote_orphans(places: &[PlaceFacts]) -> Vec<(Place, PathBuf)> {
    let mut orphaned = Vec::new();
    for found in places.iter().filter(|found| matches!(found.place.reach, Reach::Ssh(_))) {
        if let Ok(probe) = &found.probe {
            orphaned.extend(probe.orphaned.iter().map(|queue| (found.place.clone(), queue.clone())));
        }
    }
    orphaned
}

/// The GitHub repositories this host's lander units and `.safe-push` files name, as `OWNER/REPO`.
pub fn lander_repositories(config: &Path, code: &Path) -> BTreeSet<String> {
    let mut repositories = BTreeSet::new();
    for entry in fs::read_dir(config.join("systemd/user")).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("north-land@") || !name.ends_with(".service") {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else { continue };
        for line in text.lines() {
            let Some(words) = line.strip_prefix("ExecStart=") else { continue };
            let words: Vec<&str> = words.split_whitespace().collect();
            if let Some(checkout) = words.iter().position(|word| *word == "--repo").and_then(|at| words.get(at + 1)) {
                repositories.extend(github_slug(Path::new(checkout)));
            }
        }
    }
    for project in fs::read_dir(code).into_iter().flatten().flatten() {
        let checkout = project.path().join("main");
        if checkout.join(".safe-push").is_file() {
            repositories.extend(github_slug(&checkout));
        }
    }
    repositories
}

/// The GitHub repository a checkout pushes to, when its origin is GitHub.
fn github_slug(checkout: &Path) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(checkout).args(["remote", "get-url", "--push", "origin"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout);
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let path = ["https://github.com/", "ssh://git@github.com/", "git@github.com:"].iter().find_map(|prefix| url.strip_prefix(prefix))?.trim_start_matches('/');
    let mut parts = path.split('/');
    let (owner, repository) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty() && !repository.is_empty()).then(|| format!("{owner}/{repository}"))
}

/// The repositories `status` refuses to read; an error stops the check rather than naming every repository.
pub fn unreachable(repositories: &BTreeSet<String>, status: &dyn Fn(&str) -> Result<u16, String>) -> Result<Vec<String>, String> {
    let mut unreachable = Vec::new();
    for repository in repositories {
        match status(repository)? {
            code if (200..400).contains(&code) => {}
            _ => unreachable.push(repository.clone()),
        }
    }
    Ok(unreachable)
}

/// A North Lander installation token from the credential broker.
pub(super) fn lander_token() -> Result<String, String> {
    let output = Command::new(follow::program()).args(["broker", "github-token", "--app", "lander"]).stdin(Stdio::null()).output().map_err(|error| format!("north broker github-token --app lander: {error}"))?;
    let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && !token.is_empty() {
        Ok(token)
    } else {
        Err(format!("north broker github-token --app lander: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// The HTTP status GitHub answers for `repository` with `token`.
pub(super) fn github_status(token: &str, repository: &str) -> Result<u16, String> {
    let url = format!("https://api.github.com/repos/{repository}");
    let authorization = format!("Authorization: Bearer {token}");
    let output = Command::new("curl")
        .args(["-q", "--silent", "--show-error", "--output", "/dev/null", "--write-out", "%{http_code}", "--max-time", "10", "--header", authorization.as_str(), "--header", "Accept: application/vnd.github+json", url.as_str()])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("curl GET {url}: {error}"))?;
    if !output.status.success() {
        return Err(format!("curl GET {url}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let code: u16 = String::from_utf8_lossy(&output.stdout).trim().parse().map_err(|_| format!("curl GET {url} printed no HTTP status"))?;
    if code == 0 {
        return Err(format!("curl GET {url} reached no answer"));
    }
    Ok(code)
}

/// The offload hosts the lander's checks reach, as `north capacity offload hosts` names them.
pub fn offload_places() -> Vec<Place> {
    let listed = Command::new(follow::program()).args(["capacity", "offload", "hosts"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let text = listed.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
    text.lines().filter_map(|line| line.split_once(' ')).map(|(name, ssh)| Place { name: name.to_owned(), reach: Reach::Ssh(ssh.trim().to_owned()) }).collect()
}
