use std::path::{Path, PathBuf};

pub const TREE: &str = "tree";

pub fn harness_root() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().unwrap_or_default();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    resolve(std::env::var_os("NORTH_HOME").map(PathBuf::from), &executable, &home)
}

pub fn is_sha(name: &str) -> bool {
    name.len() == 40 && name.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn resolve(north_home: Option<PathBuf>, executable: &Path, home: &Path) -> Result<PathBuf, String> {
    if let Some(root) = north_home {
        return Ok(root);
    }
    if let Some(root) = own_tree(executable) {
        return Ok(root);
    }
    checkout_for(executable, home.join("code/north/main"))
}

fn own_tree(executable: &Path) -> Option<PathBuf> {
    let promoted = executable.parent().map(|runtime| runtime.join(TREE));
    promoted
        .into_iter()
        .chain(executable.ancestors().map(Path::to_path_buf))
        .find(|root| root.join("harness/lib/usage.py").is_file())
}

fn checkout_for(executable: &Path, checkout: PathBuf) -> Result<PathBuf, String> {
    let Some(runtime) = promoted_sha(executable) else {
        return Ok(checkout);
    };
    let head = head(&checkout).unwrap_or_default();
    match head == runtime {
        true => Ok(checkout),
        false => Err(format!(
            "north: runtime {runtime} has no tree and the checkout {} is at {}; run north runtime update",
            checkout.display(),
            if head.is_empty() { "no commit" } else { &head }
        )),
    }
}

pub fn revision(root: &Path) -> Option<String> {
    tree_sha(root).map(str::to_owned).or_else(|| head(root))
}

fn head(checkout: &Path) -> Option<String> {
    let output = std::process::Command::new("git").arg("-C").arg(checkout).args(["rev-parse", "HEAD"]).output().ok()?;
    let head = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !head.is_empty()).then_some(head)
}

fn tree_sha(root: &Path) -> Option<&str> {
    (root.file_name()? == TREE).then_some(())?;
    runtime_sha(root.parent()?)
}

fn promoted_sha(executable: &Path) -> Option<&str> {
    runtime_sha(executable.parent()?)
}

fn runtime_sha(runtime: &Path) -> Option<&str> {
    (runtime.parent()?.file_name()? == "runtimes").then_some(())?;
    runtime.file_name()?.to_str().filter(|name| is_sha(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn usage(root: &Path) {
        fs::create_dir_all(root.join("harness/lib")).unwrap();
        fs::write(root.join("harness/lib/usage.py"), "").unwrap();
    }

    #[test]
    fn a_promoted_binary_runs_harness_code_from_its_own_tree_not_the_checkout() {
        let home = tempfile::tempdir().unwrap();
        let runtime = home.path().join(".local/state/north/runtimes").join("a".repeat(40));
        usage(&home.path().join("code/north/main"));
        usage(&runtime.join(TREE));
        assert_eq!(resolve(None, &runtime.join("north"), home.path()), Ok(runtime.join(TREE)));
    }

    #[test]
    fn a_binary_built_in_a_worktree_runs_that_worktrees_harness_not_the_main_checkout() {
        let home = tempfile::tempdir().unwrap();
        let lane = home.path().join("code/north/worktrees/lane");
        usage(&home.path().join("code/north/main"));
        usage(&lane);
        assert_eq!(resolve(None, &lane.join("target/debug/north"), home.path()), Ok(lane.clone()));
        assert_eq!(resolve(Some("/pinned".into()), &lane.join("target/debug/north"), home.path()), Ok(PathBuf::from("/pinned")));
        assert_eq!(resolve(None, Path::new("/elsewhere/north"), home.path()), Ok(home.path().join("code/north/main")));
    }
}
