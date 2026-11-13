use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout, PROGRAMS};
use super::super::store;
use super::sync::relink;
use north_base::platform::agent_runtime;
use north_base::platform::entry_points::EntryPoint;
use rustix::fs::{Access, access};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) const ENV: &str = "NORTH_AGENT_RUNTIME_LINK";
const ROOT: &str = "agent-runtime";
const STAGING: &str = "agent-runtime-staging";
const CLAUDE_INSTALL: &str = ".local/lib/claude/current";

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
    configured().map_or(Ok(()), |link| require_writable(&link).and_then(|()| require_writable(&agent_runtime::claude_link(&link))))
}

/// The store paths one runtime published for the agent: its north programs and, when the operator has one, its Claude Code.
pub(super) struct Published {
    north: PathBuf,
    claude: Option<PathBuf>,
}

/// Adds the runtime's programs, with `worker-sandbox` linked beside its `north` for the shell prefix of an agent session, to the Nix store as one path rooted at `agent-runtime` in the runtime, and the operator's Claude Code as another rooted at `agent-claude`, so the agent runs copies no one can change in place.
pub(super) fn publish(runtime: &Path) -> Result<Option<Published>, Failure> {
    if configured().is_none() {
        return Ok(None);
    }
    let staging = runtime.join(STAGING);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    for program in PROGRAMS {
        fs::copy(runtime.join(program), staging.join(program))?;
    }
    std::os::unix::fs::symlink("north", staging.join(EntryPoint::WorkerSandbox.name()))?;
    let added = store::add_rooted(&staging, &runtime.join(ROOT), "the agent runtime");
    fs::remove_dir_all(&staging)?;
    let claude = publish_claude(runtime, operator_claude().as_deref()).inspect_err(|failure| eprintln!("north runtime update: the agent keeps the Claude Code it had: {}", failure.message)).ok();
    Ok(Some(Published { north: added?, claude }))
}

/// The version of Claude Code the operator's managed install names as current when this runtime is installed, which its own updater may have moved since the canary launched one.
fn operator_claude() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    fs::canonicalize(Path::new(&home).join(CLAUDE_INSTALL)).ok()
}

fn publish_claude(runtime: &Path, version: Option<&Path>) -> Result<PathBuf, Failure> {
    let version = version.filter(|version| version.join("bin/claude").is_file()).ok_or_else(|| failed(1, format!("no Claude Code at ~/{CLAUDE_INSTALL}/bin/claude to publish for the agent")))?;
    store::add_rooted(version, &runtime.join(agent_runtime::CLAUDE_LINK), "the agent's Claude Code")
}

/// What a runtime already published for the agent, read from its roots.
fn published_by(runtime: &Path) -> Result<Published, Failure> {
    let north = fs::read_link(runtime.join(ROOT)).map_err(|error| failed(1, format!("{} was never published for the agent: {error}", runtime.display())))?;
    Ok(Published { north, claude: fs::read_link(runtime.join(agent_runtime::CLAUDE_LINK)).ok() })
}

/// The store paths the host's agent links name, which no runtime collection may take the root of.
pub(super) fn targets() -> Vec<PathBuf> {
    configured().map(|link| targets_of(&link)).unwrap_or_default()
}

pub(super) fn targets_of(link: &Path) -> Vec<PathBuf> {
    [link.to_path_buf(), agent_runtime::claude_link(link)].iter().filter_map(|link| fs::read_link(link).ok()).collect()
}

pub(super) fn follow(layout: &Layout, published: Option<&Published>) -> Result<(), Failure> {
    follow_at(layout, configured().as_deref(), published)
}

/// A link that moves clears the record a rollback left of it, naming the runtime it left behind; a runtime with no Claude Code of its own leaves the agent's as it was.
fn follow_at(layout: &Layout, link: Option<&Path>, published: Option<&Published>) -> Result<(), Failure> {
    let (Some(link), Some(published)) = (link, published) else { return Ok(()) };
    if let Some(claude) = &published.claude {
        relink(&agent_runtime::claude_link(link), claude)?;
    }
    relink(link, &published.north)?;
    if let Ok(stuck) = fs::read_to_string(layout.agent_link_stuck()) {
        println!("north runtime: the agent runtime link follows {} again, off the runtime a rollback left it on: {}", published.north.display(), stuck.trim());
        fs::remove_file(layout.agent_link_stuck())?;
    }
    Ok(())
}

/// Moves the links back to a runtime already published, as a rollback moves `current`.
pub(super) fn roll_back(layout: &Layout, runtime: &Path, failed_sha: &str) {
    roll_back_at(layout, configured().as_deref(), runtime, failed_sha);
}

/// A link the rollback cannot move is written to `agent-link-stuck` as `FAILED_SHA WHY`, which the next update clears once it moves the link and `north land doctor` pages until then.
pub(super) fn roll_back_at(layout: &Layout, link: Option<&Path>, runtime: &Path, failed_sha: &str) {
    let Some(link) = link else { return };
    let Err(failure) = published_by(runtime).and_then(|published| follow_at(layout, Some(link), Some(&published))) else { return };
    eprintln!("north runtime rollback: the agent link stays on {}: {}", layout::short(failed_sha), failure.message);
    if let Err(error) = fs::write(layout.agent_link_stuck(), format!("{failed_sha} {}\n", failure.message)) {
        eprintln!("north runtime rollback: the stuck agent link was not recorded in {}: {error}", layout.agent_link_stuck().display());
    }
}
