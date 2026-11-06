use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use toml_edit::{DocumentMut, Item};

use north_safe_push::entry;
use crate::pacing::placement;
use crate::ops::remote_serve::south_land::{MAX_COMMITS, MAX_PATCH_BYTES};

const SOUTH: &str = "south";
const DESTINATION: &str = "main";
const BASE: &str = "refs/remotes/origin/main";
const REMOTE_COMMAND: &str = "south land";
const LEDGER_HOST_FILE: &str = ".config/threads/remote";
const USAGE: &str = "usage: north remote-land routes SAFE-PUSH-ARGUMENTS... (exits 0 when safe-push lands this checkout through the ledger host's south land, 1 when it pushes with git)";

pub struct Host {
    pub ledger_client: bool,
    pub ledger_host: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Route {
    pub ledger_host: Option<String>,
    pub dry_run: bool,
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "routes" => match ambient_route(rest) {
            Some(_) => 0,
            None => 1,
        },
        Some((flag, [])) if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

pub fn safe_push(arguments: &[String]) -> u8 {
    match ambient_route(arguments) {
        Some(route) => terminal(land(&route)),
        None => entry::run(arguments),
    }
}

pub fn route(arguments: &[String], origin_url: &str, host: &Host) -> Option<Route> {
    let entry::Destination { to, dry_run } = entry::destination(arguments)?;
    (to == DESTINATION && host.ledger_client && repository_name(origin_url) == Some(SOUTH)).then(|| Route { ledger_host: host.ledger_host.clone(), dry_run })
}

pub fn ledger_client(config: &DocumentMut, here: &str) -> bool {
    config
        .get("hosts")
        .and_then(Item::as_table_like)
        .and_then(|hosts| hosts.get(here))
        .and_then(|host| host.get("ledger_client"))
        .and_then(Item::as_bool)
        .unwrap_or(false)
}

fn repository_name(origin_url: &str) -> Option<&str> {
    let path = origin_url.trim().trim_end_matches('/');
    let name = path.rsplit(['/', ':']).next()?;
    Some(name.strip_suffix(".git").unwrap_or(name)).filter(|name| !name.is_empty())
}

fn environment() -> BTreeMap<String, String> {
    std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect()
}

fn ambient_host(environment: &BTreeMap<String, String>) -> Host {
    let ledger_client = placement::config().is_ok_and(|config| ledger_client(&config, &crate::platform::host::name()));
    let ledger_host = environment
        .get("HOME")
        .and_then(|home| fs::read_to_string(Path::new(home).join(LEDGER_HOST_FILE)).ok())
        .map(|destination| destination.trim().to_owned())
        .filter(|destination| !destination.is_empty());
    Host { ledger_client, ledger_host }
}

fn ambient_route(arguments: &[String]) -> Option<Route> {
    let checkout = std::env::current_dir().ok()?;
    let origin_url = git(&checkout, &["remote", "get-url", "origin"]).ok()?;
    route(arguments, &origin_url, &ambient_host(&environment()))
}

fn git(checkout: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").current_dir(checkout).args(arguments).stdin(Stdio::null()).output().map_err(|error| format!("git {}: {error}", arguments[0]))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!("git {} failed: {}", arguments[0], String::from_utf8_lossy(&output.stderr).trim()))
    }
}

fn terminal(outcome: Result<String, String>) -> u8 {
    match outcome {
        Ok(line) => {
            println!("safe-push: {line}");
            0
        }
        Err(reason) => {
            println!("safe-push: refused {reason}");
            1
        }
    }
}

fn land(route: &Route) -> Result<String, String> {
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
        let output = Command::new("git").current_dir(&self.checkout).args(["format-patch", "--stdout", "--no-signature", &range]).stdin(Stdio::null()).output().map_err(|error| format!("git format-patch: {error}"))?;
        if !output.status.success() {
            return Err(format!("git format-patch failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        if output.stdout.len() > MAX_PATCH_BYTES {
            return Err(format!("the {} byte patch is over the {MAX_PATCH_BYTES} byte limit of {REMOTE_COMMAND}; NOT pushed", output.stdout.len()));
        }
        Ok(Mbox { commits, bytes: output.stdout })
    }
}

fn send(destination: &str, mbox: &[u8]) -> Result<String, String> {
    let argv: Vec<OsString> = crate::platform::ssh::argv(&["ssh".into()], &environment(), &["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", destination, REMOTE_COMMAND]);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("ssh {destination} {REMOTE_COMMAND}: {error}; NOT pushed"))?;
    let written = child.stdin.take().map(|mut pipe| pipe.write_all(mbox));
    let output = child.wait_with_output().map_err(|error| format!("ssh {destination} {REMOTE_COMMAND}: {error}"))?;
    let answer = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let code = output.status.code().map_or_else(|| "a signal".to_owned(), |code| format!("exit {code}"));
        return Err(format!("{destination} {REMOTE_COMMAND} answered {code}: {}", answer.trim()));
    }
    if let Some(Err(error)) = written {
        return Err(format!("sending the patch to {destination}: {error}"));
    }
    Ok(landed_line(destination, &answer))
}

fn landed_line(destination: &str, answer: &str) -> String {
    let last = answer.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or_default();
    if last.len() == 40 && last.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        format!("landed {last}")
    } else {
        format!("landed through {destination} {REMOTE_COMMAND}")
    }
}
