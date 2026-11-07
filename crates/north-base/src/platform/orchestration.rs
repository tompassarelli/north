use std::env;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

pub fn path() -> PathBuf {
    if let Some(path) = env::var_os("AGENTS_ORCHESTRATION").filter(|path| !path.is_empty()) {
        return path.into();
    }
    match env::var_os("AGENTS_OPERATOR_DIR") {
        Some(operator) => PathBuf::from(operator).join("orchestration.toml"),
        None => under(Path::new(&env::var_os("HOME").unwrap_or_default())),
    }
}

pub fn under(home: &Path) -> PathBuf {
    home.join("code/south/main/agents/orchestration.toml")
}

pub fn read() -> Result<DocumentMut, String> {
    let path = path();
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    text.parse().map_err(|error| format!("{}: {error}", path.display()))
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
