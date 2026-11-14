use std::path::{Path, PathBuf};

use north_base::platform::session_scope::REMOVAL_ROOTS;
use north_base::platform::{git_credential, session_scope};
use north_base::foundation::offload;
use north_base::verdict_cache;
use north_providers::providers::{opencode, pi};

use super::peer::Peer;
use super::lane_grant;
use super::{CONFINED, HOSTS_HIDDEN, LANE_ONLY, MARK, SESSION_SCOPE, SOCKETS_HIDDEN};

const RUNTIME_ENV: &str = "XDG_RUNTIME_DIR";
const HOMES: [(&str, &str); 4] = [("XDG_STATE_HOME", ".local/state"), ("XDG_CACHE_HOME", ".cache"), ("XDG_DATA_HOME", ".local/share"), ("XDG_CONFIG_HOME", ".config")];
const INJECTED_NAMES: [&str; 30] = [
    "GCONV_PATH", "LOCPATH", "NLSPATH", "HOSTALIASES", "RESOLV_HOST_CONF", "GLIBC_TUNABLES", "BASH_ENV", "ENV", "NODE_OPTIONS", "SSH_ASKPASS", "GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE", "GIT_EXEC_PATH", "GIT_SSH", "GIT_SSH_COMMAND", "GIT_ASKPASS", "GIT_PAGER", "GIT_EDITOR", "GIT_EXTERNAL_DIFF", "GIT_PROXY_COMMAND", "GIT_TEMPLATE_DIR", offload::SSH_ENV, offload::GIT_ENV, offload::SUBJECT_ENV, offload::OWNER_ENV, offload::RED_FIX_ENV, offload::LANDER_SLOT_ENV, offload::HOLD_ENV, "NORTH_FLEET_DIR",
];
const INJECTED_PREFIXES: [&str; 6] = ["LD_", "MALLOC_", "PYTHON", "PERL5", "RUBY", "GIT_CONFIG"];
const TRUSTED_FILES: [&str; 5] = [verdict_cache::DIR_ENV, "AGENTS_ORCHESTRATION", "AGENTS_OPERATOR_DIR", "SAFE_PUSH_SOUTH", "SAFE_PUSH_DENYLIST"];
const NORTH_PINS: [&str; 3] = ["NORTH_LAND_NORTH", "NORTH_BIN", "AGENT_CAPACITY_NORTH"];
const LANDING_CHOICES: [&str; 12] = ["PATH", NORTH_PINS[0], NORTH_PINS[1], NORTH_PINS[2], "SAFE_PUSH_CODE_HEALTH", "SAFE_PUSH_LAND_OFFLOAD", "SAFE_PUSH_LAND_OFFLOAD_HOST", "SAFE_PUSH_LAND_RUNNER", "SAFE_PUSH_LAND_CAPACITY", offload::LOCAL_ENV, offload::OFF_HOST_ENV, offload::LANDER_SLOT_ENV];
const BUILD_NAMES: [&str; 18] = ["CARGO", "RUSTC", "RUSTDOC", "RUSTFLAGS", "RUSTDOCFLAGS", "CC", "CXX", "AR", "LD", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "CPATH", "LIBRARY_PATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "OBJC_INCLUDE_PATH"];
const BUILD_PREFIXES: [&str; 17] = ["CARGO_", "RUSTC_", "RUSTUP_", "SCCACHE_", "CMAKE_", "NEXTEST_", "CLIPPY_", "CC_", "CXX_", "AR_", "CFLAGS_", "CXXFLAGS_", "LDFLAGS_", "HOST_", "TARGET_", "PKG_CONFIG", "NIX_"];

fn builds(name: &str) -> bool {
    BUILD_NAMES.contains(&name) || BUILD_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn unsafe_name(name: &str) -> bool {
    name == MARK || name == SOCKETS_HIDDEN || name == CONFINED || name == HOSTS_HIDDEN || name == REMOVAL_ROOTS || name == SESSION_SCOPE || name == lane_grant::PROVED_ENV || name.is_empty() || name.contains('=') || INJECTED_NAMES.contains(&name) || INJECTED_PREFIXES.iter().any(|prefix| name.starts_with(prefix)) || TRUSTED_FILES.contains(&name) || [opencode::KEY_ENV, pi::KEY_ENV].contains(&name)
}

/// The programs a brokered safe-push runs, `None` for every other request: the `PATH` in `own`, the broker's environment, behind `home`'s gh shims, `north`, the broker's own build, as every north it starts, and the compiler, wrapper, flags, cache and test runner `own` sets, since the programs its checks before queueing run, the north whose `verdict hit` decides which of them run, how they build and where they run decide the verdict a ticket carries.
pub(super) fn landing_choices(arguments: &[String], home: &Path, own: &[(String, String)], north: &Path) -> Option<Vec<(String, String)>> {
    let own_path = own.iter().find(|(name, _)| name == "PATH").map_or("", |(_, value)| value.as_str());
    let north = north.to_string_lossy().into_owned();
    let path = lane_grant::shimmed_path(home, own_path).unwrap_or_else(|_| own_path.to_owned());
    arguments.first().is_some_and(|verb| verb == "safe-push").then(|| [("PATH".to_owned(), path)].into_iter().chain(NORTH_PINS.map(|pin| (pin.to_owned(), north.clone()))).chain(own.iter().filter(|(name, _)| builds(name)).cloned()).collect())
}

/// The `PATH` of a request whose verb starts no command of the caller's (`lane`, `home`, `threads`, `fleet need`), `None` for every other request: the absolute entries of the `PATH` in `own`, the broker's environment, since the git, ssh and file tools such a verb starts by name run as the operator outside the caller's domain, where a program found on the caller's `PATH` would be the caller's own (south#118). A verb that starts the caller's command (a build, a depbox run) still finds it on the caller's `PATH`.
pub(super) fn own_path(arguments: &[String], own: &[(String, String)]) -> Option<(String, String)> {
    let path = own.iter().find(|(name, _)| name == "PATH").map_or("", |(_, value)| value.as_str());
    let absolute = std::env::join_paths(std::env::split_paths(path).filter(|entry| entry.is_absolute())).map(|joined| joined.to_string_lossy().into_owned()).unwrap_or_default();
    (arguments.first().is_some_and(|verb| ["lane", "home"].contains(&verb.as_str())) || super::broker::reaches_ledger(arguments)).then(|| ("PATH".to_owned(), absolute))
}

/// The variables that name the fleet session a brokered request speaks as, `None` for every other request: a gate or a need speaks as the session the caller `peer` runs in on `host` (lane_grant.rs), and a safe-push enqueues as it (`enqueuing_as`), never as a word or variable of the client's, since a gate's question and default answer reach its session's parent and come back into that session as its decision, a need is placed for its session's next try, and a ticket is queued under its session's name. A gate or need whose `--session` names any other session is refused, as is one from a caller in no session's scope.
pub(super) fn asking_session(arguments: &[String], peer: &Peer, host: &lane_grant::Host) -> Result<Option<Vec<(String, String)>>, String> {
    let flags = match arguments {
        [noun, verb, flags @ ..] if noun == "gate" && verb == "open" => flags,
        [noun, verb, ..] if noun == "fleet" && verb == "need" => &[][..],
        [verb, ..] if verb == "safe-push" => return Ok(Some(enqueuing_as(peer, host))),
        _ => return Ok(None),
    };
    let (directory, record) = lane_grant::own_session(peer, host, "a gate or a need is asked from a fleet session's own scope, and this caller runs in none")?;
    let session = record["name"].as_str().filter(|name| !name.is_empty()).ok_or_else(|| format!("{} names no session", directory.display()))?;
    if let Some([_, other]) = flags.chunks(2).find(|pair| matches!(pair, [flag, other] if flag == "--session" && other != session)) {
        return Err(format!("{session} asks as itself, not as {other}"));
    }
    Ok(Some(vec![(lane_grant::SESSION_ENV.to_owned(), session.to_owned())]))
}

/// The variables a brokered `safe-push` enqueues under: the session the caller `peer` runs in on `host`, with `PROVED_ENV` set to say the broker read it there, or an empty session name from a caller in no session's scope, which the readers take as no session.
fn enqueuing_as(peer: &Peer, host: &lane_grant::Host) -> Vec<(String, String)> {
    let session = lane_grant::own_session(peer, host, "").ok().and_then(|(_, record)| record["name"].as_str().filter(|name| !name.is_empty()).map(str::to_owned));
    match session {
        Some(name) => vec![(lane_grant::SESSION_ENV.to_owned(), name), (lane_grant::PROVED_ENV.to_owned(), "1".to_owned())],
        None => vec![(lane_grant::SESSION_ENV.to_owned(), String::new())],
    }
}

/// `variables` without the session and proof names the client sent, with `asker`'s in their place; unchanged when `asker` is `None`.
pub(super) fn speaking_as(variables: Vec<(String, String)>, asker: Option<Vec<(String, String)>>) -> Vec<(String, String)> {
    match asker {
        Some(pairs) => variables.into_iter().filter(|(name, _)| name.as_str() != lane_grant::SESSION_ENV && name.as_str() != lane_grant::PROVED_ENV).chain(pairs).collect(),
        None => variables,
    }
}

/// The XDG homes a brokered command reads and writes: this broker's own, since the command runs as the host's process and its state, cache and data belong under the host's homes. Taking the client's scratch ones would record a build's capacity admissions beside the worker instead of in the host's decision ledger (north#1030).
pub(super) fn homes(home: Option<PathBuf>, get: impl Fn(&str) -> Option<PathBuf>) -> Vec<(String, String)> {
    let home = home.unwrap_or_else(|| PathBuf::from("/"));
    HOMES
        .into_iter()
        .map(|(name, below)| (name.to_owned(), get(name).unwrap_or_else(|| home.join(below)).to_string_lossy().into_owned()))
        .collect()
}

pub(super) fn own_homes() -> Vec<(String, String)> {
    fn set(name: &str) -> Option<PathBuf> {
        std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
    }
    homes(set("HOME"), set).into_iter().chain(set("HOME").map(|home| ("HOME".to_owned(), home.to_string_lossy().into_owned()))).collect()
}

/// The homes a request's process gets: `own`, the broker's (`homes`), apart from a `safe-push` a caller outside every lane-only scope asked for, which keeps the XDG homes the caller set, since the state home holds the landing queue its ticket belongs in, as when it ran in place before it ran through the broker (north#1094): a verify-north worker's ticket went to the host's queue instead of its scratch one, and the walk found none (north#1022). A kept state home other than the broker's does not move the verdict store, which stays the broker's, since the checks before queueing reuse a pass they find there in place of running.
pub(super) fn request_homes(arguments: &[String], peer: &Peer, sent: &[(String, String)], own: Vec<(String, String)>) -> Vec<(String, String)> {
    let queues = arguments.first().is_some_and(|verb| verb == "safe-push") && !peer.cgroup().is_none_or(session_scope::lane_only_cgroup);
    let client = |name: &str| sent.iter().find(|(sent, value)| sent == name && !value.is_empty()).map(|(_, value)| value.clone());
    let state = own.iter().find(|(name, _)| name == "XDG_STATE_HOME").map(|(_, value)| value.clone());
    let moved = queues && client("XDG_STATE_HOME").is_some_and(|kept| Some(kept) != state);
    let store = state.filter(|_| moved).map(|state| (verdict_cache::DIR_ENV.to_owned(), Path::new(&state).join("north/verdict-cache").to_string_lossy().into_owned()));
    own.into_iter().map(|(name, value)| match (queues && name != "HOME").then(|| client(&name)).flatten() {
        Some(kept) => (name, kept),
        None => (name, value),
    }).chain(store).collect()
}

/// The environment a request's process gets: the client's, without the sandbox mark, the variables that make a loader, interpreter or git run code of the client's choosing (git's program, editor, template and configuration variables), the variables that name a file or store a guard on the landing path trusts (the verdict store, the orchestration policy, the privacy denylist's sources), the paid key an opencode or pi session's commands must not read, and relative or empty `PATH` entries; git's credential helper is north's own, and `XDG_RUNTIME_DIR` is `runtime`, the broker's own, since the command runs outside and reaches the user manager and the capacity database there whatever the client set or left out (an OpenCode worker clears it, north#1023). The XDG homes and `HOME` are `homes`, the broker's own apart from a safe-push's state home (`request_homes`): a client's scratch homes would take the state, cache and data a brokered build writes with it (north#1030), and `north home move` takes the broker's `HOME` as the top of its moves (north#992). Its lane-only marker follows the caller `peer`'s cgroup, the cgroup of the process that asked (`None` when unreadable, which counts as lane-only), never the client's word: a caller in a lane-only session's scope gets the marker whether it sent one or not, and any other caller keeps one it sent, since the marker only narrows what its command reads (south#104).
pub(super) fn environment(sent: Vec<(String, String)>, peer: &Peer, runtime: &Path, homes: &[(String, String)]) -> Vec<(String, String)> {
    let lane_only = peer.cgroup().is_none_or(session_scope::lane_only_cgroup);
    let path = |value: &str| std::env::join_paths(std::env::split_paths(value).filter(|entry| entry.is_absolute())).map(|joined| joined.to_string_lossy().into_owned()).unwrap_or_default();
    let own_runtime = (RUNTIME_ENV.to_owned(), runtime.to_string_lossy().into_owned());
    sent.into_iter().filter(|(name, _)| !unsafe_name(name) && name != RUNTIME_ENV && name != "HOME" && !HOMES.iter().any(|(own, _)| *own == name)).map(|(name, value)| if name == "PATH" { (name, path(&value)) } else { (name, value) }).chain(git_credential::session_env()).chain(lane_only.then(|| (LANE_ONLY.to_owned(), "1".to_owned()))).chain([own_runtime]).chain(homes.iter().cloned()).collect()
}

/// The environment a request's process gets: for a worker's `capacity host status` or `backlog`, the broker's own `own` alone, without the sandbox's markers and with the broker's runtime directory and homes, since each reads and tidies the host's admission state where the broker's environment, never the client's, says it lives; for every other request, the client's `sent` as `environment` makes it.
pub(super) fn environment_for(arguments: &[String], sent: Vec<(String, String)>, peer: &Peer, runtime: &Path, homes: &[(String, String)], own: &[(String, String)]) -> Vec<(String, String)> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    if !matches!(words.as_slice(), ["capacity", "host", "status" | "backlog"]) {
        return environment(sent, peer, runtime, homes);
    }
    let own_runtime = (RUNTIME_ENV.to_owned(), runtime.to_string_lossy().into_owned());
    own.iter().filter(|(name, _)| !unsafe_name(name) && name != RUNTIME_ENV && name != "HOME" && !HOMES.iter().any(|(home, _)| *home == name)).cloned().chain([own_runtime]).chain(homes.iter().cloned()).collect()
}

/// `variables` for a request whose programs the broker chose (`landing_choices`): without the client's `PATH`, its build environment and the variables that pick the north, the gate programs and the host and runner its checks before queueing use, then with the broker's choices.
pub(super) fn landing(variables: Vec<(String, String)>, chosen: Option<Vec<(String, String)>>) -> Vec<(String, String)> {
    match chosen {
        Some(chosen) => variables.into_iter().filter(|(name, _)| !LANDING_CHOICES.contains(&name.as_str()) && !builds(name)).chain(chosen).collect(),
        None => variables,
    }
}

/// `variables` with the fleet session `NORTH_FLEET_SESSION` names taken from the caller `peer`, the asking process's, never the client's word: the spool a brokered command may write (state.rs) and the session its rows and calls go out as belong to the session whose own scope or held unit the caller runs in ([`lane_grant::own_session`]), and a caller in none gets no session.
pub(super) fn caller_session(variables: Vec<(String, String)>, peer: &Peer, host: &lane_grant::Host) -> Vec<(String, String)> {
    let session = lane_grant::own_session(peer, host, "").ok().and_then(|(_, record)| record["name"].as_str().filter(|name| !name.is_empty()).map(str::to_owned));
    variables.into_iter().filter(|(name, _)| name != lane_grant::SESSION_ENV).chain(session.map(|name| (lane_grant::SESSION_ENV.to_owned(), name))).collect()
}

pub(super) fn urgency(variables: Vec<(String, String)>, peer: &Peer, host: &lane_grant::Host) -> Vec<(String, String)> {
    let places = crate::capacity::host::urgency::Places::beside(host.fleet.clone(), host.home.clone(), host.cgroups.clone());
    let urgent = peer.cgroup().is_some_and(|cgroup| places.cgroup(cgroup));
    variables.into_iter().filter(|(name, _)| name != offload::URGENT_ENV).chain(urgent.then(|| (offload::URGENT_ENV.to_owned(), "1".to_owned()))).collect()
}

#[cfg(test)]
#[path = "urgency_tests.rs"]
mod urgency_tests;
