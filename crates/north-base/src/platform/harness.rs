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
        .find(|root| root.join("harness/defaults.toml").is_file())
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
