use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::error::{NorthError, NorthResult};

use super::configuration;
use super::paths::owner_path;

const LIST: &str = "harness/retired-hooks.txt";
const HOOKS: &str = "harness/hooks/";
const STUB: &str = "#!/bin/sh\nexit 0\n";

fn names(text: &str) -> impl Iterator<Item = &str> {
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
        if !name.contains('/') && !retired.contains(&name) {
            return configuration(format!(
                "{path} is deleted but {name} is not listed in {LIST}; hosts that still register it would fail with 'No such file or directory' until rebuilt"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn git(root: &Path, arguments: &[&str]) {
        let status = Command::new("git").arg("-C").arg(root).args(arguments).status().unwrap();
        assert!(status.success(), "git {arguments:?}");
    }

    #[test]
    fn stubs_cover_retired_names_without_replacing_live_hooks() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("live.sh"), "real").unwrap();
        write_stubs_for(directory.path(), "gone.sh\n\nlive.sh\n").unwrap();
        assert_eq!(fs::read_to_string(directory.path().join("gone.sh")).unwrap(), STUB);
        assert_eq!(fs::metadata(directory.path().join("gone.sh")).unwrap().permissions().mode() & 0o111, 0o111);
        assert_eq!(fs::read_to_string(directory.path().join("live.sh")).unwrap(), "real");
    }

    #[test]
    fn deleting_a_hook_script_requires_listing_it_as_retired() {
        let repo = tempfile::tempdir().unwrap();
        let root = repo.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.test"]);
        git(root, &["config", "user.name", "t"]);
        fs::create_dir_all(root.join("harness/hooks")).unwrap();
        fs::write(root.join("harness/hooks/old.sh"), "x").unwrap();
        fs::write(root.join(LIST), "").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "base"]);
        git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(root, &["rm", "-q", "harness/hooks/old.sh"]);
        git(root, &["commit", "-q", "-m", "delete"]);
        let error = require_deleted_hooks_retired(root, "HEAD").expect_err("unlisted deletion must refuse");
        assert!(error.to_string().contains("old.sh"));
        fs::write(root.join(LIST), "old.sh\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "retire"]);
        require_deleted_hooks_retired(root, "HEAD").unwrap();
    }
}
