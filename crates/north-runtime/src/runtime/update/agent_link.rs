use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout, PROGRAMS};
use super::super::store;
use super::sync::relink;
use rustix::fs::{Access, access};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) const ENV: &str = "NORTH_AGENT_RUNTIME_LINK";
const ROOT: &str = "agent-runtime";
const STAGING: &str = "agent-runtime-staging";

#[cfg(not(test))]
const NAMING_FILE: &str = north_base::platform::agent_runtime::NAMING_FILE;
#[cfg(test)]
const NAMING_FILE: &str = "/nonexistent/north/agent-runtime-link";
const ROOT_UID: u32 = 0;

/// The operator-owned link the agent user follows to the live runtime's store path; a host that names none keeps every runtime path as it was.
pub(super) fn configured() -> Option<PathBuf> {
    chosen(std::env::var_os(ENV), Path::new(NAMING_FILE), ROOT_UID)
}

pub(super) fn chosen(env: Option<OsString>, file: &Path, owner: u32) -> Option<PathBuf> {
    env.filter(|value| !value.is_empty()).map(PathBuf::from).or_else(|| named_in(file, owner))
}

/// The first line of a naming file only the host's root can write; a file anyone else owns or can change names nothing.
fn named_in(file: &Path, owner: u32) -> Option<PathBuf> {
    let opened = fs::File::open(file).ok()?;
    let meta = opened.metadata().ok()?;
    if !meta.is_file() || meta.uid() != owner || meta.mode() & 0o022 != 0 {
        return None;
    }
    let line = BufReader::new(opened).lines().next()?.ok()?;
    let path = PathBuf::from(line.trim());
    path.is_absolute().then_some(path)
}

/// Refuses a caller that could not move the agent's link, so an update never swaps `current` and leaves the agent on the runtime it replaced.
pub(crate) fn require_writable(link: &Path) -> Result<(), Failure> {
    let directory = link.parent().unwrap_or(Path::new("."));
    if access(directory, Access::WRITE_OK).is_err() {
        return Err(failed(1, format!("this user cannot write the agent runtime link {}; only its owner updates the runtime here", link.display())));
    }
    if fs::symlink_metadata(link).is_ok_and(|meta| !meta.file_type().is_symlink()) {
        return Err(failed(1, format!("{} is not a link; the agent runtime link is left as it is", link.display())));
    }
    Ok(())
}

pub(crate) fn guard() -> Result<(), Failure> {
    configured().map_or(Ok(()), |link| require_writable(&link))
}

/// Adds the runtime's programs to the Nix store as one path rooted at `agent-runtime` in the runtime, so the agent runs a copy no one can change in place.
pub(super) fn publish(runtime: &Path) -> Result<Option<PathBuf>, Failure> {
    if configured().is_none() {
        return Ok(None);
    }
    let staging = runtime.join(STAGING);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    for program in PROGRAMS {
        fs::copy(runtime.join(program), staging.join(program))?;
    }
    let added = store::add_rooted(&staging, &runtime.join(ROOT), "the agent runtime");
    fs::remove_dir_all(&staging)?;
    added.map(Some)
}

/// The store paths the host's agent links name, which no runtime collection may take the root of.
pub(super) fn targets() -> Vec<PathBuf> {
    configured().and_then(|link| fs::read_link(link).ok()).into_iter().collect()
}

pub(super) fn follow(layout: &Layout, published: Option<&Path>) -> Result<(), Failure> {
    follow_at(layout, configured().as_deref(), published)
}

/// A link that moves clears the record a rollback left of it, naming the runtime it left behind.
fn follow_at(layout: &Layout, link: Option<&Path>, published: Option<&Path>) -> Result<(), Failure> {
    let (Some(link), Some(path)) = (link, published) else { return Ok(()) };
    relink(link, path)?;
    if let Ok(stuck) = fs::read_to_string(layout.agent_link_stuck()) {
        println!("north runtime: the agent runtime link follows {} again, off the runtime a rollback left it on: {}", path.display(), stuck.trim());
        fs::remove_file(layout.agent_link_stuck())?;
    }
    Ok(())
}

/// Moves the link back to a runtime already published, as a rollback moves `current`.
pub(super) fn roll_back(layout: &Layout, runtime: &Path, failed_sha: &str) {
    roll_back_at(layout, configured().as_deref(), runtime, failed_sha);
}

/// A link the rollback cannot move is written to `agent-link-stuck` as `FAILED_SHA WHY`, which the next update clears once it moves the link and `north land doctor` pages until then.
pub(super) fn roll_back_at(layout: &Layout, link: Option<&Path>, runtime: &Path, failed_sha: &str) {
    let Some(link) = link else { return };
    let published = fs::read_link(runtime.join(ROOT)).map_err(|error| failed(1, format!("{} was never published for the agent: {error}", runtime.display())));
    let Err(failure) = published.and_then(|published| follow_at(layout, Some(link), Some(&published))) else { return };
    eprintln!("north runtime rollback: the agent link stays on {}: {}", layout::short(failed_sha), failure.message);
    if let Err(error) = fs::write(layout.agent_link_stuck(), format!("{failed_sha} {}\n", failure.message)) {
        eprintln!("north runtime rollback: the stuck agent link was not recorded in {}: {error}", layout.agent_link_stuck().display());
    }
}
