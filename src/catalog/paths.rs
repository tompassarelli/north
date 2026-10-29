use std::env;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::platform::harness;

use super::{configuration, string_field};

pub(super) fn operator_catalog_path() -> NorthResult<PathBuf> {
    let configured = env::var("NORTH_REPO_ROOTS").ok();
    let directory = operator_dir(env::var_os("AGENTS_OPERATOR_DIR"), |repo| {
        repo_root_in(repo, configured.as_deref())
    })?;
    Ok(directory.join("catalog-config.json"))
}

pub(super) fn operator_dir(
    configured: Option<std::ffi::OsString>,
    root: impl Fn(&str) -> NorthResult<PathBuf>,
) -> NorthResult<PathBuf> {
    if let Some(directory) = configured.filter(|directory| !directory.is_empty()) {
        return Ok(PathBuf::from(directory));
    }
    let south = root("south")?.join("agents");
    if south.is_dir() {
        return Ok(south);
    }
    configuration(format!(
        "no operator directory: set AGENTS_OPERATOR_DIR or check out {}",
        south.display()
    ))
}

pub(crate) fn repo_root(repo: &str) -> NorthResult<PathBuf> {
    repo_root_in(repo, env::var("NORTH_REPO_ROOTS").ok().as_deref())
}

pub(super) fn repo_root_in(repo: &str, configured: Option<&str>) -> NorthResult<PathBuf> {
    if let Some(configured) = configured {
        let roots: Value = serde_json::from_str(configured)?;
        if let Some(path) = roots.get(repo).and_then(Value::as_str) {
            return Ok(PathBuf::from(path));
        }
    }
    if repo == "north" {
        return harness::harness_root().map_err(NorthError::Configuration);
    }
    Ok(home()?.join("code").join(repo).join("main"))
}

pub(super) fn owner_path(owner: &Value) -> NorthResult<PathBuf> {
    owner_path_in(owner, env::var("NORTH_REPO_ROOTS").ok().as_deref())
}

fn owner_path_in(owner: &Value, configured: Option<&str>) -> NorthResult<PathBuf> {
    let root = repo_root_in(string_field(owner, "repo")?, configured)?;
    let relative = Path::new(string_field(owner, "path")?);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| component.as_os_str() == "..")
    {
        return configuration("owner path escapes its repository");
    }
    let path = root.join(relative);
    if !path.exists() {
        return configuration(format!("owner source does not exist: {}", path.display()));
    }
    Ok(path)
}

pub(super) fn agents_root() -> NorthResult<PathBuf> {
    Ok(env::var_os("NORTH_AGENT_STATE_ROOT")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".local/state/north/agents")))
}

pub(super) fn home() -> NorthResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| NorthError::Configuration("HOME is unavailable".into()))
}
