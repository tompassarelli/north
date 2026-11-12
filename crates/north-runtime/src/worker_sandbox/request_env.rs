use std::path::{Path, PathBuf};

use north_base::platform::session_scope::REMOVAL_ROOTS;
use north_base::platform::{git_credential, session_scope};
use north_base::foundation::offload;
use north_base::verdict_cache;

use super::lane_grant;
use super::{LANE_ONLY, MARK, SESSION_SCOPE, SOCKETS_HIDDEN};

const RUNTIME_ENV: &str = "XDG_RUNTIME_DIR";
const HOMES: [(&str, &str); 4] = [("XDG_STATE_HOME", ".local/state"), ("XDG_CACHE_HOME", ".cache"), ("XDG_DATA_HOME", ".local/share"), ("XDG_CONFIG_HOME", ".config")];
const INJECTED_NAMES: [&str; 19] = [
    "GCONV_PATH", "LOCPATH", "NLSPATH", "HOSTALIASES", "RESOLV_HOST_CONF", "GLIBC_TUNABLES", "BASH_ENV", "ENV", "NODE_OPTIONS", "SSH_ASKPASS", "GIT_EXEC_PATH", "GIT_SSH", "GIT_SSH_COMMAND", "GIT_ASKPASS", "GIT_PAGER", "GIT_EDITOR", "GIT_EXTERNAL_DIFF", "GIT_PROXY_COMMAND", "GIT_TEMPLATE_DIR",
];
const INJECTED_PREFIXES: [&str; 6] = ["LD_", "MALLOC_", "PYTHON", "PERL5", "RUBY", "GIT_CONFIG"];
const TRUSTED_FILES: [&str; 5] = [verdict_cache::DIR_ENV, "AGENTS_ORCHESTRATION", "AGENTS_OPERATOR_DIR", "SAFE_PUSH_SOUTH", "SAFE_PUSH_DENYLIST"];
const NORTH_PINS: [&str; 3] = ["NORTH_LAND_NORTH", "NORTH_BIN", "AGENT_CAPACITY_NORTH"];
const LANDING_CHOICES: [&str; 12] = ["PATH", NORTH_PINS[0], NORTH_PINS[1], NORTH_PINS[2], "SAFE_PUSH_CODE_HEALTH", "SAFE_PUSH_LAND_OFFLOAD", "SAFE_PUSH_LAND_OFFLOAD_HOST", "SAFE_PUSH_LAND_RUNNER", "SAFE_PUSH_LAND_CAPACITY", offload::LOCAL_ENV, offload::OFF_HOST_ENV, offload::LANDER_SLOT_ENV];
const BUILD_NAMES: [&str; 10] = ["CARGO", "RUSTC", "RUSTDOC", "RUSTFLAGS", "RUSTDOCFLAGS", "CC", "CXX", "CFLAGS", "CXXFLAGS", "LDFLAGS"];
const BUILD_PREFIXES: [&str; 6] = ["CARGO_", "RUSTC_", "SCCACHE_", "CMAKE_", "NEXTEST_", "CLIPPY_"];

fn builds(name: &str) -> bool {
    BUILD_NAMES.contains(&name) || BUILD_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn unsafe_name(name: &str) -> bool {
    name == MARK || name == SOCKETS_HIDDEN || name == REMOVAL_ROOTS || name == SESSION_SCOPE || name.is_empty() || name.contains('=') || INJECTED_NAMES.contains(&name) || INJECTED_PREFIXES.iter().any(|prefix| name.starts_with(prefix)) || TRUSTED_FILES.contains(&name)
}

/// The programs a brokered safe-push runs, `None` for every other request: the `PATH` in `own`, the broker's environment, behind `home`'s gh shims, `north`, the broker's own build, as every north it starts, and the compiler, wrapper, flags, cache and test runner `own` sets, since the programs its checks before queueing run, the north whose `verdict hit` decides which of them run, how they build and where they run decide the verdict a ticket carries.
pub(super) fn landing_choices(arguments: &[String], home: &Path, own: &[(String, String)], north: &Path) -> Option<Vec<(String, String)>> {
    let own_path = own.iter().find(|(name, _)| name == "PATH").map_or("", |(_, value)| value.as_str());
    let north = north.to_string_lossy().into_owned();
    let path = lane_grant::shimmed_path(home, own_path).unwrap_or_else(|_| own_path.to_owned());
    arguments.first().is_some_and(|verb| verb == "safe-push").then(|| [("PATH".to_owned(), path)].into_iter().chain(NORTH_PINS.map(|pin| (pin.to_owned(), north.clone()))).chain(own.iter().filter(|(name, _)| builds(name)).cloned()).collect())
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
pub(super) fn request_homes(arguments: &[String], cgroup: Option<&str>, sent: &[(String, String)], own: Vec<(String, String)>) -> Vec<(String, String)> {
    let queues = arguments.first().is_some_and(|verb| verb == "safe-push") && !cgroup.is_none_or(session_scope::lane_only_cgroup);
    let client = |name: &str| sent.iter().find(|(sent, value)| sent == name && !value.is_empty()).map(|(_, value)| value.clone());
    let state = own.iter().find(|(name, _)| name == "XDG_STATE_HOME").map(|(_, value)| value.clone());
    let moved = queues && client("XDG_STATE_HOME").is_some_and(|kept| Some(kept) != state);
    let store = state.filter(|_| moved).map(|state| (verdict_cache::DIR_ENV.to_owned(), Path::new(&state).join("north/verdict-cache").to_string_lossy().into_owned()));
    own.into_iter().map(|(name, value)| match (queues && name != "HOME").then(|| client(&name)).flatten() {
        Some(kept) => (name, kept),
        None => (name, value),
    }).chain(store).collect()
}

/// The environment a request's process gets: the client's, without the sandbox mark, the variables that make a loader, interpreter or git run code of the client's choosing (git's program, editor, template and configuration variables), the variables that name a file or store a guard on the landing path trusts (the verdict store, the orchestration policy, the privacy denylist's sources), and relative or empty `PATH` entries; git's credential helper is north's own, and `XDG_RUNTIME_DIR` is `runtime`, the broker's own, since the command runs outside and reaches the user manager and the capacity database there whatever the client set or left out (an OpenCode worker clears it, north#1023). The XDG homes and `HOME` are `homes`, the broker's own apart from a safe-push's state home (`request_homes`): a client's scratch homes would take the state, cache and data a brokered build writes with it (north#1030), and `north home move` takes the broker's `HOME` as the top of its moves (north#992). Its lane-only marker follows `cgroup`, the cgroup of the process that asked (`None` when unreadable, which counts as lane-only), never the client's word: a caller in a lane-only session's scope gets the marker whether it sent one or not, and any other caller keeps one it sent, since the marker only narrows what its command reads (south#104).
pub(super) fn environment(sent: Vec<(String, String)>, cgroup: Option<&str>, runtime: &Path, homes: &[(String, String)]) -> Vec<(String, String)> {
    let lane_only = cgroup.is_none_or(session_scope::lane_only_cgroup);
    let path = |value: &str| std::env::join_paths(std::env::split_paths(value).filter(|entry| entry.is_absolute())).map(|joined| joined.to_string_lossy().into_owned()).unwrap_or_default();
    let own_runtime = (RUNTIME_ENV.to_owned(), runtime.to_string_lossy().into_owned());
    sent.into_iter().filter(|(name, _)| !unsafe_name(name) && name != RUNTIME_ENV && name != "HOME" && !HOMES.iter().any(|(own, _)| *own == name)).map(|(name, value)| if name == "PATH" { (name, path(&value)) } else { (name, value) }).chain(git_credential::session_env()).chain(lane_only.then(|| (LANE_ONLY.to_owned(), "1".to_owned()))).chain([own_runtime]).chain(homes.iter().cloned()).collect()
}

/// `variables` for a request whose programs the broker chose (`landing_choices`): without the client's `PATH`, its build environment and the variables that pick the north, the gate programs and the host and runner its checks before queueing use, then with the broker's choices.
pub(super) fn landing(variables: Vec<(String, String)>, chosen: Option<Vec<(String, String)>>) -> Vec<(String, String)> {
    match chosen {
        Some(chosen) => variables.into_iter().filter(|(name, _)| !LANDING_CHOICES.contains(&name.as_str()) && !builds(name)).chain(chosen).collect(),
        None => variables,
    }
}
