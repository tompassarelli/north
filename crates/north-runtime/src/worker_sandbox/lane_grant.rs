//! The verbs the outside broker runs with the host's GitHub credentials for a lane-only session (DeepSeek's harnesses, which hold no token: south#104), so it lands and reports without one (north#1056): `safe-push --to main` from its own lane, which enqueues that lane for the lander, and a comment on, or the Done-when checker's tick of a box of, its own task issue. The session is the fleet session whose scope the caller's cgroup names, never anything the request says; its lane is `PROJECT/worktrees/NAME` beside the `main` it was launched in (or the lane it was launched in), and its task the `--task` of its launch. A request's words choose only among those verbs; any other ref, repository, issue or option is refused. The command runs with the broker's own environment, never the client's, with git's hooks, fsmonitor and ssh command off whatever the repository's configuration says, and refuses a lane whose git configuration names a filter driver or `core.sshCommand`, and with GitHub reached through the gh shim as the Agent App under the session's name; its output reaches the client with every token-shaped word withheld, and no descriptor goes back but the client's own streams.

use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use north_base::platform::session_scope::{self, LANE_ONLY_UNIT_PREFIX, REMOVAL_ROOTS, SESSION_UNIT_PREFIX};
use north_base::platform::{gh_shim, git_credential, session_host_files};
use regex::Regex;
use serde_json::Value;

use super::peer::Peer;
use super::{CONFINED, LANE_ONLY, MARK, SOCKETS_HIDDEN};

const DESTINATION: &str = "main";
pub(super) const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
pub(super) const PROVED_ENV: &str = "NORTH_FLEET_SESSION_PROVED";
const SHIM_DIRECTORIES: [&str; 2] = [".local/bin", ".local/share/north/bin"];
pub(super) const GIT_HARDENING: [(&str, &str); 3] = [("core.hooksPath", "/dev/null"), ("core.fsmonitor", "false"), ("core.sshCommand", "false")];
const ADOPTED_WINDOW: &str = "adopted window, process ";
const COMMAND_KEYS: &str = r"^(filter\..+\.(clean|smudge|process)|core\.sshcommand)$";
const WITHHELD: &str = "[token withheld]";
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:gh[pousr]_|github_pat_)[A-Za-z0-9_]{16,}").expect("the token pattern compiles"));

/// The host this broker serves: where the fleet of the state root the broker was started under keeps its session records (`NORTH_FLEET_DIR`, else `north/fleet` under `XDG_STATE_HOME`, as the fleet itself places them, so a broker verify-north starts for its scratch state root finds its worker there), the home whose gh shim the command reaches GitHub through, the directories that hold the projects it serves requests from (`~/code`, and the one a broker was started to serve beside it), the broker's own environment a granted command starts from, and where the cgroup filesystem is mounted.
pub(super) struct Host {
    pub fleet: PathBuf,
    pub home: PathBuf,
    pub code: Vec<PathBuf>,
    pub environment: Vec<(String, String)>,
    pub cgroups: PathBuf,
}

impl Host {
    pub fn own() -> Self {
        let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|state| state.is_absolute()).unwrap_or_else(|| home.join(".local/state"));
        let fleet = std::env::var_os("NORTH_FLEET_DIR").filter(|fleet| !fleet.is_empty()).map_or_else(|| state.join("north/fleet"), PathBuf::from);
        let environment = std::env::vars_os().filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?))).collect();
        Self { fleet, code: vec![home.join("code")], home, environment, cgroups: PathBuf::from("/sys/fs/cgroup") }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Session {
    pub name: String,
    pub task: String,
    pub lane: PathBuf,
}

/// Whether `arguments` ask for a verb this module grants a lane-only session.
pub(super) fn privileged(arguments: &[String]) -> bool {
    match arguments {
        [verb, ..] if verb == "safe-push" => true,
        [noun, verb, ..] => noun == "task" && (verb == "comment" || verb == "check"),
        _ => false,
    }
}

/// Whether the request `arguments` from the caller `peer` is served by this module's grant: each verb it grants, apart from a `safe-push` from outside every lane-only session's scope. That caller (a Claude worker) holds its own token, and the broker runs its safe-push with its own environment, outside the Landlock domain that refuses the mount namespaces its checks before queueing build (north#1094).
pub(super) fn granted(arguments: &[String], peer: &Peer) -> bool {
    privileged(arguments) && (arguments.first().is_none_or(|verb| verb != "safe-push") || peer.cgroup().is_none_or(north_base::platform::session_scope::lane_only_cgroup))
}

/// The fleet session whose scope the caller `peer` runs in names, when it is a lane-only one.
pub(super) fn session(peer: &Peer, fleet: &Path) -> Result<Session, String> {
    let unit = peer.cgroup().unwrap_or_default()
        .lines()
        .flat_map(|line| line.split('/'))
        .find(|leaf| leaf.starts_with(LANE_ONLY_UNIT_PREFIX) && leaf.ends_with(".scope"))
        .ok_or("only a lane-only session asks the broker to land or write its issue; any other session holds its own token")?;
    let (directory, record) = recorded(unit, fleet)?;
    let field = |key: &str| record[key].as_str().filter(|value| !value.is_empty()).map(str::to_owned);
    let name = field("name").ok_or_else(|| format!("{} names no session", directory.display()))?;
    let task = field("task_ref").ok_or_else(|| format!("{name} was launched with no --task, so it has no issue of its own"))?;
    let lane = own_lane(&name, &record)?;
    Ok(Session { name, task, lane })
}

/// The lane of the session `name` whose record is `record`: the working directory it launched in when that is a lane, else the lane of its own name beside that directory.
pub(super) fn own_lane(name: &str, record: &Value) -> Result<PathBuf, String> {
    let cwd = PathBuf::from(record["cwd"].as_str().filter(|value| !value.is_empty()).ok_or_else(|| format!("{name} records no working directory"))?);
    if cwd.parent().and_then(Path::file_name).is_some_and(|parent| parent == "worktrees") {
        Ok(cwd)
    } else {
        Ok(cwd.parent().ok_or_else(|| format!("{} is not a checkout of a project", cwd.display()))?.join("worktrees").join(name))
    }
}

/// The directory under `fleet` of the session that runs in the scope `unit`, and its record.
pub(super) fn recorded(unit: &str, fleet: &Path) -> Result<(PathBuf, Value), String> {
    let directory = std::fs::read_dir(fleet)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|directory| session_host_files::scope(directory).as_deref() == Some(unit))
        .ok_or_else(|| format!("no fleet session in {} runs in {unit}", fleet.display()))?;
    let record = std::fs::read(directory.join("session.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or_else(|| format!("{} has no readable session.json", directory.display()))?;
    Ok((directory, record))
}

/// The directory and record of the fleet session the caller `peer` runs in, or `absent` when it runs in none: a session scope this host's fleet started, else an adopted window whose process the caller is or descends from, or a held service another host placed a session's CLI in here, whose session is the placed record of the conversation that host bound the unit to in the unit's own directory (remote.rs). The caller chooses neither its unit nor what the launcher wrote, and a placed lane-only session is refused, since its held unit carries no lane-only mark to grant it by (north#1054).
pub(super) fn own_session(peer: &Peer, host: &Host, absent: &str) -> Result<(PathBuf, Value), String> {
    let cgroup = peer.cgroup().unwrap_or_default();
    let leaves = || cgroup.lines().flat_map(|line| line.split('/'));
    if let Some(unit) = leaves().find(|leaf| leaf.starts_with(SESSION_UNIT_PREFIX) && leaf.ends_with(".scope")) {
        return recorded(unit, &host.fleet);
    }
    if let Some(adopted) = adopted_window(peer.pids(), &host.fleet) {
        return Ok(adopted);
    }
    let (unit, conversation) = leaves().filter(|leaf| leaf.ends_with(".service")).find_map(|unit| Some((unit, bound_conversation(&host.home.join(session_host_files::HELD).join(unit))?))).ok_or(absent)?;
    let directory = host.fleet.join(session_host_files::PLACED).join(&conversation);
    let record = read_record(&directory).ok_or_else(|| format!("{unit} runs the conversation {conversation}, and no record of it was placed here"))?;
    if session_scope::session_unit(record["provider"].as_str().unwrap_or_default(), "").starts_with(LANE_ONLY_UNIT_PREFIX) {
        return Err(format!("{} is a lane-only session placed here by another host, and its held unit carries no lane-only mark to grant it by", record["name"].as_str().unwrap_or(unit)));
    }
    Ok((directory, record))
}

/// The directory and record of the adopted session whose window process is one of `pids`, the caller or an ancestor of it: an adopted record names the window process it bound in its placement (`adopted window, process N`), and a shell run in that window descends from it.
fn adopted_window(pids: &[u32], fleet: &Path) -> Option<(PathBuf, Value)> {
    std::fs::read_dir(fleet).into_iter().flatten().flatten().map(|entry| entry.path()).find_map(|directory| {
        let record = read_record(&directory)?;
        let process = record["placement"].as_str()?.strip_prefix(ADOPTED_WINDOW)?.parse::<u32>().ok()?;
        (!record["adopted_ms"].is_null() && pids.contains(&process)).then_some((directory, record))
    })
}

/// The placed sessions whose held unit runs here now, by directory and record: each held directory bound to a conversation whose CLI has not recorded its exit and whose unit has a cgroup under the host's, since a retired session's stopped unit never records an exit and its held directory and placed record stay behind.
pub(super) fn placed_running(host: &Host) -> Vec<(PathBuf, Value)> {
    let active = active_units(&host.cgroups);
    let held = std::fs::read_dir(host.home.join(session_host_files::HELD)).into_iter().flatten().flatten().map(|entry| entry.path());
    held.filter(|unit| !unit.join("status").exists() && unit.file_name().is_some_and(|name| active.contains(name)))
        .filter_map(|unit| bound_conversation(&unit))
        .map(|conversation| host.fleet.join(session_host_files::PLACED).join(conversation))
        .filter_map(|directory| read_record(&directory).map(|record| (directory, record)))
        .collect()
}

const CGROUP_DEPTH: usize = 6;

/// The names of the cgroup directories under `root` within the depth systemd nests a user unit at, one of which a unit has exactly while it is active, read in one walk for every held unit.
fn active_units(root: &Path) -> std::collections::HashSet<std::ffi::OsString> {
    fn below(directory: &Path, depth: usize, names: &mut std::collections::HashSet<std::ffi::OsString>) {
        for entry in std::fs::read_dir(directory).into_iter().flatten().flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())) {
            if depth > 0 {
                below(&entry.path(), depth - 1, names);
            }
            names.insert(entry.file_name());
        }
    }
    let mut names = std::collections::HashSet::new();
    below(root, CGROUP_DEPTH, &mut names);
    names
}

/// The conversation the launching host bound the held unit whose directory is `held` to.
fn bound_conversation(held: &Path) -> Option<String> {
    let conversation = std::fs::read_to_string(held.join(session_host_files::BOUND_CONVERSATION)).ok()?.trim().to_owned();
    (!conversation.is_empty() && conversation.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')).then_some(conversation)
}

fn read_record(directory: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(directory.join("session.json")).ok()?).ok()
}

/// The `north` arguments that serve `arguments`, asked from `directory` by `session`, or why they are refused.
pub(super) fn grant(session: &Session, arguments: &[String], directory: &Path) -> Result<Vec<String>, String> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["safe-push", rest @ ..] => land(session, rest, directory),
        ["task", "comment", rest @ ..] => comment(session, rest),
        ["task", "check", rest @ ..] => tick(session, rest),
        _ => Err(format!("{} is not a verb the broker runs for a lane-only session", arguments.join(" "))),
    }
}

fn land(session: &Session, rest: &[&str], directory: &Path) -> Result<Vec<String>, String> {
    let inside = directory.canonicalize().ok().zip(session.lane.canonicalize().ok()).is_some_and(|(directory, lane)| directory.starts_with(lane));
    if !inside {
        return Err(format!("{} is not inside {}, the lane of {}; a lane-only session lands only its own lane", directory.display(), session.lane.display(), session.name));
    }
    refuse_command_config(session)?;
    let mut granted = vec!["safe-push".to_owned(), "--to".to_owned(), DESTINATION.to_owned()];
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match (*word, words.clone().next()) {
            ("--to", Some(&DESTINATION)) => {
                words.next();
            }
            ("--to", destination) => return Err(format!("a lane-only session lands only on its repository's {DESTINATION} through the lander, not on {}", destination.unwrap_or(&"nothing"))),
            ("--keep-lane", _) => granted.push("--keep-lane".to_owned()),
            ("--several-commits", Some(reason)) => {
                granted.extend(["--several-commits".to_owned(), (*reason).to_owned()]);
                words.next();
            }
            (other, _) => return Err(format!("safe-push {other} is not granted to a lane-only session; it lands with safe-push --to {DESTINATION} [--several-commits REASON] [--keep-lane]")),
        }
    }
    Ok(granted)
}

/// Refuses a lane whose own git configuration (the repository's or the lane's) names a filter driver or `core.sshCommand`, which git would run with the broker's credentials when it lands the lane.
fn refuse_command_config(session: &Session) -> Result<(), String> {
    for scope in ["--local", "--worktree"] {
        let found = std::process::Command::new("git")
            .arg("-C")
            .arg(&session.lane)
            .args(["config", scope, "--name-only", "--get-regexp", COMMAND_KEYS])
            .env_remove("GIT_DIR")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .map_err(|error| format!("git config in {}: {error}", session.lane.display()))?;
        let keys = String::from_utf8_lossy(&found.stdout);
        if let Some(key) = keys.lines().next() {
            return Err(format!("{} sets {key} in its git configuration, which git would run with the broker's credentials; remove it before landing", session.lane.display()));
        }
    }
    Ok(())
}

fn comment(session: &Session, rest: &[&str]) -> Result<Vec<String>, String> {
    let mut body = None;
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match *word {
            "--body" => body = words.next().filter(|text| !text.trim().is_empty()),
            named if named.contains('#') => own_issue(session, named)?,
            other => return Err(format!("task comment {other} is not granted; a lane-only session comments with north task comment [ISSUE] --body TEXT")),
        }
    }
    let body = body.ok_or("task comment needs --body TEXT")?;
    Ok(["task", "comment", &session.task, "--body", body].map(str::to_owned).to_vec())
}

fn tick(session: &Session, rest: &[&str]) -> Result<Vec<String>, String> {
    let (mut done_when, mut dry_run, mut only) = (false, false, None);
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match *word {
            "--done-when" => done_when = true,
            "--dry-run" => dry_run = true,
            "--box" => only = words.next().filter(|text| !text.trim().is_empty()),
            named if named.contains('#') => own_issue(session, named)?,
            other => return Err(format!("task check {other} is not granted; a lane-only session ticks with north task check --done-when --box TEXT [ISSUE]")),
        }
    }
    let only = only.filter(|_| done_when).ok_or("a lane-only session ticks one box its Done-when checker verifies: north task check --done-when --box TEXT")?;
    let mut granted: Vec<String> = ["task", "check", "--done-when", "--box", only, &session.task].map(str::to_owned).to_vec();
    if dry_run {
        granted.push("--dry-run".to_owned());
    }
    Ok(granted)
}

fn own_issue(session: &Session, named: &str) -> Result<(), String> {
    if same_issue(named, &session.task) { Ok(()) } else { Err(format!("{named} is not {}, the task of {}; a lane-only session writes only its own issue", session.task, session.name)) }
}

/// Whether the reference `named` (`OWNER/REPO#N`, `REPO#N` or `#N`) names the issue `task` does.
pub(super) fn same_issue(named: &str, task: &str) -> bool {
    let parts = |reference: &str| {
        let (repository, number) = reference.split_once('#')?;
        let (owner, name) = repository.rsplit_once('/').map_or((None, repository), |(owner, name)| (Some(owner.to_owned()), name));
        Some((owner, name.to_owned(), number.parse::<u64>().ok()?))
    };
    let (Some((named_owner, named_name, named_number)), Some((task_owner, task_name, task_number))) = (parts(named), parts(task)) else { return false };
    let own_repository = (named_name.is_empty() && named_owner.is_none()) || (named_name == task_name && (named_owner.is_none() || task_owner.is_none() || named_owner == task_owner));
    named_number == task_number && own_repository
}

/// The environment a granted command runs with: the broker's `own`, without the sandbox's markers, its git configuration or its `PATH`, then a `PATH` that finds the gh shim first, the session's name for the shim to call GitHub as the Agent App under, and git's credential helper with hooks and fsmonitor off. Refused when `home` holds no gh shim, since gh would then answer as the operator's own login.
pub(super) fn environment(session: &Session, own: impl IntoIterator<Item = (String, String)>, home: &Path) -> Result<Vec<(String, String)>, String> {
    let mut path = None;
    let mut kept: Vec<(String, String)> = Vec::new();
    for (name, value) in own {
        match name.as_str() {
            "PATH" => path = Some(value),
            MARK | LANE_ONLY | SOCKETS_HIDDEN | CONFINED | REMOVAL_ROOTS | SESSION_ENV | PROVED_ENV => {}
            _ if name.starts_with("GIT_CONFIG") => {}
            _ => kept.push((name, value)),
        }
    }
    kept.push(("PATH".to_owned(), shimmed_path(home, path.as_deref().unwrap_or_default())?));
    kept.push((SESSION_ENV.to_owned(), session.name.clone()));
    kept.push((PROVED_ENV.to_owned(), "1".to_owned()));
    kept.extend(git_credential::session_env_with(&GIT_HARDENING));
    Ok(kept)
}

/// `path`'s absolute entries behind `home`'s gh shim directories; refused when `home` holds no gh shim, since gh would then answer as the operator's own login.
pub(super) fn shimmed_path(home: &Path, path: &str) -> Result<String, String> {
    let shims: Vec<PathBuf> = SHIM_DIRECTORIES.iter().map(|directory| home.join(directory)).filter(|directory| gh_shim::is_shim(&directory.join("gh"))).collect();
    if shims.is_empty() {
        return Err(format!("no north gh shim in {} or {}, so gh would answer as a personal login", home.join(SHIM_DIRECTORIES[0]).display(), home.join(SHIM_DIRECTORIES[1]).display()));
    }
    let joined = std::env::join_paths(shims.into_iter().chain(std::env::split_paths(path).filter(|entry| entry.is_absolute()))).map_err(|error| error.to_string())?;
    Ok(joined.to_string_lossy().into_owned())
}

/// `text` with every token-shaped word withheld.
pub(super) fn withheld(text: &str) -> Cow<'_, str> {
    TOKEN.replace_all(text, WITHHELD)
}

/// Copies `from` to `to` line by line with every token-shaped word withheld.
pub(super) fn relay(from: impl Read, mut to: impl Write) {
    let mut reader = BufReader::new(from);
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
        if to.write_all(withheld(&String::from_utf8_lossy(&line)).as_bytes()).and_then(|()| to.flush()).is_err() {
            return;
        }
        line.clear();
    }
}

#[cfg(test)]
#[path = "lane_grant_tests.rs"]
mod tests;
