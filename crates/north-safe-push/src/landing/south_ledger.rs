use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::south_land_limits::{MAX_COMMITS, MAX_PATCH_BYTES};

use crate::report::terminal::{Terminal, receipt};
use crate::request::{Destination, destination};
use crate::shell::ledger::{self, git, patch};

const SOUTH: &str = "south";
const DESTINATION: &str = "main";
const BASE: &str = "refs/remotes/origin/main";
const REMOTE_COMMAND: &str = "south land";
const LEDGER_HOST_FILE: &str = ".config/threads/remote";

pub struct HostFacts {
    pub ledger_client: bool,
    pub abandon: fn(&[String]) -> u8,
}

struct Host {
    ledger_client: bool,
    ledger_host: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Route {
    ledger_host: Option<String>,
    dry_run: bool,
}

pub fn land(route: &Route) -> u8 {
    let (line, code) = receipt(&deliver(route));
    println!("{line}");
    code
}

fn route(arguments: &[String], origin_url: &str, host: &Host) -> Option<Route> {
    let Destination { to, dry_run } = destination(arguments)?;
    (to == DESTINATION && host.ledger_client && repository_name(origin_url) == Some(SOUTH)).then(|| Route { ledger_host: host.ledger_host.clone(), dry_run })
}

fn repository_name(origin_url: &str) -> Option<&str> {
    let path = origin_url.trim().trim_end_matches('/');
    let name = path.rsplit(['/', ':']).next()?;
    Some(name.strip_suffix(".git").unwrap_or(name)).filter(|name| !name.is_empty())
}

fn environment() -> BTreeMap<String, String> {
    std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect()
}

fn ambient_host(facts: &HostFacts, environment: &BTreeMap<String, String>) -> Host {
    let ledger_host = environment
        .get("HOME")
        .and_then(|home| fs::read_to_string(Path::new(home).join(LEDGER_HOST_FILE)).ok())
        .map(|destination| destination.trim().to_owned())
        .filter(|destination| !destination.is_empty());
    Host { ledger_client: facts.ledger_client, ledger_host }
}

pub fn ambient_route(arguments: &[String], facts: &HostFacts) -> Option<Route> {
    let checkout = std::env::current_dir().ok()?;
    let origin_url = git(&checkout, &["remote", "get-url", "origin"]).ok()?;
    route(arguments, &origin_url, &ambient_host(facts, &environment()))
}

fn deliver(route: &Route) -> Terminal {
    match transfer(route) {
        Ok(line) if route.dry_run => Terminal::Clean(line),
        Ok(line) => Terminal::Queued(line),
        Err(reason) => Terminal::Refused(reason),
    }
}

fn transfer(route: &Route) -> Result<String, String> {
    let destination = route.ledger_host.as_deref().ok_or_else(|| format!("this host is a ledger client but ~/{LEDGER_HOST_FILE} names no ledger host to run {REMOTE_COMMAND} on; NOT pushed"))?;
    let checkout = std::env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let mbox = Lane::at(checkout).mbox()?;
    if route.dry_run {
        return Ok(format!("✓ {} would go to {destination} {REMOTE_COMMAND} (--dry-run)", mbox.summary()));
    }
    send(destination, &mbox.bytes)
}

struct Lane {
    checkout: PathBuf,
}

struct Mbox {
    commits: usize,
    bytes: Vec<u8>,
}

impl Mbox {
    fn summary(&self) -> String {
        format!("{} commit{} ({} bytes)", self.commits, if self.commits == 1 { "" } else { "s" }, self.bytes.len())
    }
}

impl Lane {
    fn at(checkout: PathBuf) -> Self {
        Self { checkout }
    }

    fn mbox(&self) -> Result<Mbox, String> {
        git(&self.checkout, &["rev-parse", "--verify", "-q", BASE]).map_err(|_| format!("{BASE} is missing, so the lane has no base to send; NOT pushed"))?;
        if !git(&self.checkout, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
            return Err("the worktree has uncommitted changes; commit them first; NOT pushed".to_owned());
        }
        let range = format!("{BASE}..HEAD");
        let commits: usize = git(&self.checkout, &["rev-list", "--count", &range])?.parse().map_err(|_| "git rev-list --count gave no number".to_owned())?;
        if commits == 0 {
            return Err(format!("HEAD has no commits beyond origin/{DESTINATION}; nothing to land"));
        }
        if commits > MAX_COMMITS {
            return Err(format!("{commits} commits is over the {MAX_COMMITS} commit limit of {REMOTE_COMMAND}; NOT pushed"));
        }
        let bytes = patch(&self.checkout, &range)?;
        if bytes.len() > MAX_PATCH_BYTES {
            return Err(format!("the {} byte patch is over the {MAX_PATCH_BYTES} byte limit of {REMOTE_COMMAND}; NOT pushed", bytes.len()));
        }
        Ok(Mbox { commits, bytes })
    }
}

fn send(destination: &str, mbox: &[u8]) -> Result<String, String> {
    let answer = ledger::send(destination, REMOTE_COMMAND, mbox, &environment())?;
    if !answer.success {
        let code = answer.code.map_or_else(|| "a signal".to_owned(), |code| format!("exit {code}"));
        return Err(format!("{destination} {REMOTE_COMMAND} answered {code}: {}", answer.stdout.trim()));
    }
    if let Some(error) = answer.write_error {
        return Err(format!("sending the patch to {destination}: {error}"));
    }
    Ok(landed_line(destination, &answer.stdout))
}

fn landed_line(destination: &str, answer: &str) -> String {
    let last = answer.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or_default();
    if last.len() == 40 && last.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        format!("landed {last}")
    } else {
        format!("landed through {destination} {REMOTE_COMMAND}")
    }
}

#[cfg(test)]
#[path = "south_ledger_tests.rs"]
mod tests;
