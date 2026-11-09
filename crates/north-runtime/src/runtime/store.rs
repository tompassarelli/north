use super::failure::{Failure, failed};
use super::layout::PROGRAMS;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const ROOT_PREFIX: &str = "gcroot-";

pub(super) fn references(runtime: &Path) -> Result<BTreeSet<String>, Failure> {
    let mut references = BTreeSet::new();
    for program in PROGRAMS {
        references.extend(crate::platform::store_refs::roots(&fs::read(runtime.join(program))?));
    }
    Ok(references)
}

pub(super) fn root(runtime: &Path) -> Result<(), Failure> {
    for reference in references(runtime)? {
        let name = reference.rsplit('/').next().unwrap_or_default();
        let status = Command::new("nix-store")
            .arg("--add-root")
            .arg(runtime.join(format!("{ROOT_PREFIX}{name}")))
            .args(["--indirect", "--realise", &reference])
            .stdout(Stdio::null())
            .status()
            .map_err(|error| failed(1, format!("nix-store did not run: {error}")))?;
        if !status.success() {
            return Err(failed(1, format!("rooting {reference} failed: {status}")));
        }
    }
    Ok(())
}


/// Adds the tree's capacity-gate shims (`bin/cargo` and every name linked to it) to the Nix store as one path and roots it at `shims`, so the operator's terminal links them from the store of the runtime's landed commit (south#93).
pub(super) fn shims(runtime: &Path) -> Result<(), Failure> {
    let tree = runtime.join(super::layout::TREE).join("bin");
    let staging = runtime.join("capacity-gate-shims");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(staging.join("bin"))?;
    fs::copy(tree.join("cargo"), staging.join("bin/cargo"))?;
    for entry in fs::read_dir(&tree)?.flatten() {
        if fs::read_link(entry.path()).is_ok_and(|target| target == Path::new("cargo")) {
            std::os::unix::fs::symlink("cargo", staging.join("bin").join(entry.file_name()))?;
        }
    }
    let added = Command::new("nix-store").arg("--add").arg(&staging).stderr(Stdio::inherit()).output().map_err(|error| failed(1, format!("nix-store did not run: {error}")))?;
    fs::remove_dir_all(&staging)?;
    let path = String::from_utf8_lossy(&added.stdout).trim().to_owned();
    if !added.status.success() || path.is_empty() {
        return Err(failed(1, format!("adding the capacity-gate shims to the store failed: {}", added.status)));
    }
    let rooted = Command::new("nix-store").arg("--add-root").arg(runtime.join("shims")).args(["--indirect", "--realise", &path]).stdout(Stdio::null()).status().map_err(|error| failed(1, format!("nix-store did not run: {error}")))?;
    if !rooted.success() {
        return Err(failed(1, format!("rooting {path} failed: {rooted}")));
    }
    Ok(())
}
