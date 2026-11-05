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

