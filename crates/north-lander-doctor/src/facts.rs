use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::platform::land_queue;
use north_landing::land::after::follow;
use north_landing::land::candidate::git;
use north_landing::land::{link_hold, pause, post_land, queue, watchdog};

pub const INFRASTRUCTURE_WINDOW_SECS: u64 = 2 * 3600;
pub const DECIDED_WINDOW_SECS: u64 = 24 * 3600;
pub const PROBE_SECONDS: &str = "60";
pub const INFRASTRUCTURE: &[&str] = &["is a missing but already registered worktree", "is not on this host", "lacks the commits under", "No space left on device"];
pub const TOOLCHAINS: &[&str] = &["gcc", "binutils", "glibc", "clang", "llvm", "rustc", "ghc"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub ssh: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub queues: Vec<QueueFacts>,
    pub link: Option<LinkFacts>,
    pub places: Vec<PlaceFacts>,
    pub unserved: Vec<String>,
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
    pub held: Option<u64>,
    pub lag: Option<u64>,
    pub green_since_link: bool,
    pub refused: Option<String>,
    pub builds: BTreeSet<String>,
    pub units: Vec<String>,
    pub unreached: Option<link_hold::Unreached>,
}

#[derive(Clone, Debug)]
pub struct PlaceFacts {
    pub place: Place,
    pub probe: Result<Probe, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub free_percent: f64,
    pub free_gib: f64,
    pub slots: Vec<SlotHold>,
    pub prunable: Vec<(PathBuf, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotHold {
    pub slot: String,
    pub held: u64,
    pub pgid: Option<u32>,
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
    let unit = land_queue::serving_unit(repo).unwrap_or_else(|| watchdog::unit(repo));
    let pending = queue::pending(queue).unwrap_or_default();
    let oldest_queued = pending.iter().map(|(path, _)| u64::try_from(queue::queued_micros(path) / 1_000_000).unwrap_or(u64::MAX)).min().map(|queued| now.saturating_sub(queued));
    let decided = decided(&queue.join("done"), now);
    let since_landing = decided.iter().filter(|decision| decision.verdict.starts_with("landed ")).map(|decision| decision.age).min();
    let since_decision = decided.iter().map(|decision| decision.age).min();
    let infrastructure = decided
        .iter()
        .filter(|decision| decision.age <= INFRASTRUCTURE_WINDOW_SECS && decision.verdict.starts_with("failed "))
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
        infrastructure,
        source_builds: source_builds(&queue.join("logs"), now),
    }
}

struct Decided {
    age: u64,
    verdict: String,
    branch: String,
    path: PathBuf,
}

fn decided(done: &Path, now: u64) -> Vec<Decided> {
    let Ok(entries) = fs::read_dir(done) else { return Vec::new() };
    let mut decided: Vec<Decided> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .filter_map(|path| Some((age(&path, now)?, path)))
        .filter(|(age, _)| *age <= DECIDED_WINDOW_SECS)
        .filter_map(|(age, path)| {
            let ticket = queue::read(&path).ok()?;
            Some(Decided { age, verdict: ticket.verdict?, branch: ticket.branch, path })
        })
        .collect();
    decided.sort_by_key(|decision| decision.age);
    let mut newest = BTreeSet::new();
    decided.retain(|decision| newest.insert(decision.branch.clone()));
    decided
}

pub fn reran(queue: &Path, ticket: &Path) -> bool {
    ticket.file_name().is_some_and(|name| queue.join(crate::RERAN).join(name).exists())
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
    let held = (lander != current).then(|| link_hold::since(home).map_or_else(|| link_age(&home.join("current"), now).unwrap_or(0), |since| now.saturating_sub(since)));
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
    Some(LinkFacts { home: home.to_owned(), green_since_link: green_since(&post_land::ledger_path(), now.saturating_sub(link_set)), lander, current, held, lag, refused, builds, units, unreached: link_hold::unreached_of(home) })
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

pub const PROBE: &str = r#"set -u
root=${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}
now=$(date +%s)
df -Pk "$HOME" | awk 'NR == 2 { print "disk", $3, $4 }'
for h in "$root"/*.holder; do
  [ -f "$h" ] || continue
  base=${h%.holder}
  case $base in *.background) continue ;; esac
  if (exec 7>>"$base.lock"; flock -n 7) 2>/dev/null; then continue; fi
  pgid=$(cat "$base.pgid" 2>/dev/null || true)
  echo "slot ${base##*/} $((now - $(stat -c %Y "$h"))) ${pgid:--}"
done
for g in "$HOME"/code/*/main/.git; do
  [ -d "$g" ] || continue
  n=$(git -C "${g%/.git}" worktree list --porcelain 2>/dev/null | grep -c '^prunable' || true)
  [ "$n" = 0 ] || echo "prunable $n ${g%/.git}"
done
"#;

pub fn parse_probe(text: &str) -> Result<Probe, String> {
    let mut probe = Probe::default();
    let mut disk = None;
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["disk", used, available] => {
                let (used, available): (f64, f64) = (used.parse().map_err(|_| format!("disk line {line:?}"))?, available.parse().map_err(|_| format!("disk line {line:?}"))?);
                disk = Some((available * 100.0 / (used + available).max(1.0), available / (1u64 << 20) as f64));
            }
            ["slot", slot, held, pgid] => probe.slots.push(SlotHold { slot: (*slot).to_owned(), held: held.parse().map_err(|_| format!("slot line {line:?}"))?, pgid: pgid.parse().ok().filter(|pgid| *pgid > 1) }),
            ["prunable", count, repo] => probe.prunable.push((PathBuf::from(repo), count.parse().map_err(|_| format!("prunable line {line:?}"))?)),
            _ => {}
        }
    }
    let (free_percent, free_gib) = disk.ok_or_else(|| "the probe printed no disk line".to_owned())?;
    probe.free_percent = free_percent;
    probe.free_gib = free_gib;
    Ok(probe)
}

/// Runs `script` with `arguments` under bash on `place`: here, or over ssh with the script on stdin.
pub fn shell(place: &Place, script: &str, arguments: &[&str]) -> Result<String, String> {
    let mut command = Command::new("timeout");
    command.arg(PROBE_SECONDS);
    match &place.ssh {
        Some(ssh) => command.args(["ssh", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", "bash", "-s", "--"]),
        None => command.args(["bash", "-s", "--"]),
    };
    let mut child = command.args(arguments).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("{}: {error}", place.name))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).map_err(|error| format!("{}: {error}", place.name))?;
    }
    let output = child.wait_with_output().map_err(|error| format!("{}: {error}", place.name))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() { Ok(stdout) } else { Err(format!("{} exited {}: {}", place.name, output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim())) }
}

pub fn place_facts(place: Place) -> PlaceFacts {
    let probe = shell(&place, PROBE, &[]).and_then(|text| parse_probe(&text));
    PlaceFacts { place, probe }
}

/// The offload hosts the lander's checks reach, as `north capacity offload hosts` names them.
pub fn offload_places() -> Vec<Place> {
    let listed = Command::new(follow::program()).args(["capacity", "offload", "hosts"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let text = listed.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
    text.lines().filter_map(|line| line.split_once(' ')).map(|(name, ssh)| Place { name: name.to_owned(), ssh: Some(ssh.trim().to_owned()) }).collect()
}
