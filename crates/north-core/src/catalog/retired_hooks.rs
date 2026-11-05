use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::error::{NorthError, NorthResult};

use super::configuration;
use super::paths::owner_path;

pub(super) const LIST: &str = "harness/retired-hooks.txt";
const HOOKS: &str = "harness/hooks/";
const STUB: &str = "#!/bin/sh\nexit 0\n";

pub(super) fn names(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|line| !line.is_empty())
}

pub(super) fn write_stubs(provider_hooks: &Path) -> NorthResult<()> {
    let Ok(list) = owner_path(&json!({"repo": "north", "path": LIST})) else {
        return Ok(());
    };
    write_stubs_for(provider_hooks, &fs::read_to_string(list)?)
}

fn write_stubs_for(provider_hooks: &Path, list: &str) -> NorthResult<()> {
    for name in names(list) {
        let stub = provider_hooks.join(name);
        if stub.exists() {
            continue;
        }
        fs::write(&stub, STUB)?;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub(super) fn require_deleted_hooks_retired(root: &Path, head: &str) -> NorthResult<()> {
    let git = |arguments: &[&str]| -> NorthResult<String> {
        let output = Command::new("git").arg("-C").arg(root).args(arguments).output()?;
        if !output.status.success() {
            return configuration(format!("git {} failed in {}", arguments.join(" "), root.display()));
        }
        String::from_utf8(output.stdout).map_err(|error| NorthError::Configuration(error.to_string()))
    };
    let base = git(&["merge-base", head, "origin/main"])?;
    let range = format!("{}..{head}", base.trim());
    let deleted = git(&["diff", "--name-only", "--diff-filter=D", &range, "--", HOOKS])?;
    let list = git(&["show", &format!("{head}:{LIST}")])?;
    let retired: Vec<&str> = names(&list).collect();
    for path in deleted.lines() {
        let name = path.strip_prefix(HOOKS).unwrap_or(path);
        let registrable = !name.contains('/') && !name.contains(".test.");
        if registrable && !retired.contains(&name) {
            return configuration(format!(
                "{path} is deleted but {name} is not listed in {LIST}; hosts that still register it would fail with 'No such file or directory' until rebuilt"
            ));
        }
    }
    Ok(())
}
