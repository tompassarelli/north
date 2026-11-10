use std::env;
use std::path::{Path, PathBuf};

use north_shared_config as shared;
use toml_edit::{DocumentMut, Item};

const FILE: &str = "orchestration.toml";

pub fn path() -> PathBuf {
    if let Some(path) = configured() {
        return path;
    }
    let home = home();
    let snapshot = shared::current(&home).join(FILE);
    match snapshot.is_file() {
        true => snapshot,
        false => under(&home),
    }
}

pub fn under(home: &Path) -> PathBuf {
    home.join("code/south/main/agents/orchestration.toml")
}

/// The orchestration policy at the fetched south revision; once a host has a snapshot, one whose last fetch is past
/// the bound is refused rather than read.
pub fn read() -> Result<DocumentMut, String> {
    let home = home();
    if configured().is_none() && shared::current(&home).is_dir() {
        shared::fresh(&home, shared::now())?;
    }
    let path = path();
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    text.parse().map_err(|error| format!("{}: {error}", path.display()))
}

fn configured() -> Option<PathBuf> {
    if let Some(path) = env::var_os("AGENTS_ORCHESTRATION").filter(|path| !path.is_empty()) {
        return Some(path.into());
    }
    env::var_os("AGENTS_OPERATOR_DIR").map(|operator| PathBuf::from(operator).join(FILE))
}

fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap_or_default())
}

/// The names of the `[hosts.NAME]` tables that carry an `ssh` destination: the machines a lander reaches to pull an outbox from.
pub fn ssh_hosts(cfg: &DocumentMut) -> Vec<String> {
    let hosts = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter());
    hosts.filter(|(_, host)| host.get("ssh").and_then(Item::as_str).is_some()).map(|(name, _)| name.to_owned()).collect()
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
