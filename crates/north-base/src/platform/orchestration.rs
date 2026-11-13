use std::env;
use std::path::{Path, PathBuf};

use north_shared_config as shared;
use toml_edit::{DocumentMut, Item};

pub use shared::orchestration as path_in;

pub fn path() -> PathBuf {
    path_in(&home(), variable("AGENTS_ORCHESTRATION").as_deref(), variable("AGENTS_OPERATOR_DIR").as_deref())
}

/// A top-level policy file of south `agents/` beside the orchestration file, such as costs.toml or
/// claude-accounts.toml, so every policy a run reads comes from the same revision.
pub fn policy_file(file: &str) -> PathBuf {
    path().with_file_name(file)
}

/// Whether this home keeps south at all: a shared-config snapshot or a south checkout, either of which is there unless reading it says it is not found.
pub fn south_kept() -> bool {
    let home = home();
    let absent = |path: PathBuf| std::fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
    !(absent(shared::current(&home)) && absent(shared::checkout(&home)))
}

pub fn under(home: &Path) -> PathBuf {
    shared::checkout(home).join("agents/orchestration.toml")
}

/// The orchestration policy at the fetched south revision; once a host has a snapshot, one whose last fetch is past
/// the bound is refused rather than read.
pub fn read() -> Result<DocumentMut, String> {
    let home = home();
    if variable("AGENTS_ORCHESTRATION").is_none() && variable("AGENTS_OPERATOR_DIR").is_none() && shared::current(&home).is_dir() {
        shared::fresh(&home, shared::now())?;
    }
    parsed(&path())
}

/// The orchestration file the shared-config snapshot holds, else on a host with no snapshot yet the south checkout's, whatever the environment names.
pub fn snapshot_path() -> PathBuf {
    path_in(&home(), None, None)
}

/// The orchestration policy a trust decision reads: the shared-config snapshot under the stale-snapshot refusal.
pub fn snapshot() -> Result<DocumentMut, String> {
    let home = home();
    if shared::current(&home).is_dir() {
        shared::fresh(&home, shared::now())?;
    }
    parsed(&snapshot_path())
}

fn parsed(path: &Path) -> Result<DocumentMut, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    text.parse().map_err(|error| format!("{}: {error}", path.display()))
}

fn variable(name: &str) -> Option<PathBuf> {
    env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap_or_default())
}

pub fn agent_login(cfg: &DocumentMut, host: &str) -> Option<String> {
    login(cfg, host, "agent_ssh")
}

/// The home of the Unix user `login` logs in as, None when it names no user name or names the operator, whose home is `operator_home`.
pub fn login_home(login: &str, operator_home: &Path) -> Option<PathBuf> {
    let user = login.split_once('@').map(|(user, _)| user).filter(|user| user.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_') && user.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))?;
    let home = PathBuf::from(format!("/home/{user}"));
    (home != operator_home).then_some(home)
}

/// The operator's ssh destination `[hosts.NAME].ssh`, which operator work dials: the lander's fetch, outbox and private-kind verdict ask for every lane outside the agent user's home.
pub fn operator_login(cfg: &DocumentMut, host: &str) -> Option<String> {
    login(cfg, host, "ssh")
}

fn login(cfg: &DocumentMut, host: &str, key: &str) -> Option<String> {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)?.get(key)?.as_str()).map(str::to_owned)
}

/// The names of the `[hosts.NAME]` tables that carry an `ssh` destination: the machines a lander reaches to pull an outbox from.
pub fn ssh_hosts(cfg: &DocumentMut) -> Vec<String> {
    let hosts = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter());
    hosts.filter(|(_, host)| host.get("ssh").and_then(Item::as_str).is_some()).map(|(name, _)| name.to_owned()).collect()
}

/// The machine Tom types on: the `[hosts.NAME]` table with no `ssh` destination whose `serves` names `laptop`.
pub fn owners_machine(path: &Path) -> Option<String> {
    let cfg: DocumentMut = std::fs::read_to_string(path).ok()?.parse().ok()?;
    let hosts = cfg.get("hosts").and_then(Item::as_table_like)?;
    let laptop = |host: &Item| host.get("serves").and_then(Item::as_array).is_some_and(|serves| serves.iter().any(|need| need.as_str() == Some("laptop")));
    hosts.iter().find(|(_, host)| host.get("ssh").is_none() && laptop(host)).map(|(name, _)| name.to_owned())
}

/// The `[hosts.NAME]` tables that take other hosts' builds (`offload_builds = true`), with the ssh destination each names.
pub fn build_hosts(cfg: &DocumentMut) -> Vec<(String, String)> {
    let hosts = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter());
    hosts
        .filter(|(_, host)| host.get("offload_builds").and_then(Item::as_bool) == Some(true))
        .filter_map(|(name, host)| Some((name.to_owned(), host.get("ssh")?.as_str()?.to_owned())))
        .collect()
}

/// Whether `[hosts.HOST] runs_no_checks = true` closes the host to land checks and offloaded builds.
pub fn runs_no_checks(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("runs_no_checks")).and_then(Item::as_bool) == Some(true)
}

/// Whether `[hosts.HOST]` builds north with Buck2 (`build_system = "buck2"`, the key the build seam reads); a host without the key builds with Cargo.
pub fn builds_with_buck2(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)?.get("build_system")?.as_str()) == Some("buck2")
}

/// Every repo of a project with `paused = true`, its `repos` and `dispatch_repos` both.
pub fn set_aside_repos(cfg: &DocumentMut) -> Vec<String> {
    let listed = |item: Option<&Item>| item.and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect::<Vec<_>>();
    let projects = cfg.get("projects").and_then(Item::as_table_like).into_iter().flat_map(|projects| projects.iter());
    let mut repos: Vec<String> = projects
        .filter(|(_, project)| project.get("paused").and_then(Item::as_bool).unwrap_or(false))
        .flat_map(|(_, project)| listed(project.get("repos")).into_iter().chain(listed(project.get("dispatch_repos"))))
        .collect();
    repos.sort();
    repos.dedup();
    repos
}

pub fn placed_hosts(path: &Path) -> Option<Vec<String>> {
    let cfg: DocumentMut = std::fs::read_to_string(path).ok()?.parse().ok()?;
    let hosts: Vec<String> = cfg.get("hosts")?.as_table_like()?.iter().map(|(name, _)| name.to_owned()).collect();
    (!hosts.is_empty()).then_some(hosts)
}

/// Whether `here` can open ssh to `host`: itself, or a host its `reaches` list names; a host that declares no `reaches` list reaches only itself.
pub fn reaches(cfg: &DocumentMut, here: &str, host: &str) -> bool {
    host == here || cfg.get("hosts").and_then(|hosts| hosts.get(here)?.get("reaches")?.as_array()).is_some_and(|names| names.iter().any(|name| name.as_str() == Some(host)))
}

/// Each `[projects.NAME]` that declares `status_command = ["PROGRAM", "ARG", ...]`: the command whose first line of output the fleet digest shows for it.
pub fn status_commands(cfg: &DocumentMut) -> Vec<(String, Vec<String>)> {
    let projects = cfg.get("projects").and_then(Item::as_table_like).into_iter().flat_map(|projects| projects.iter());
    projects
        .filter_map(|(name, project)| {
            let argv = project.get("status_command")?.as_array()?;
            Some((name.to_owned(), argv.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()))
        })
        .collect()
}
