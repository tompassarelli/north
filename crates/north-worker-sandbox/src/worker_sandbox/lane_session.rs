use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Context;
use super::lane_only::SCRATCH_ENV;
use super::removable::{lane, project};

const MARK: &str = "lane-domain";

fn marker() -> Option<PathBuf> {
    std::env::var_os(SCRATCH_ENV).filter(|scratch| !scratch.is_empty()).map(|scratch| PathBuf::from(scratch).join(MARK))
}

/// Records, in the session's scratch directory, the directory its long-lived domain was built for: the file's creation time is when the domain was built, a kernel fact the session cannot set back, and its content is the session's lane or project directory.
pub(super) fn begin(cwd: &Path) {
    if let Some(marker) = marker() {
        let _ = std::fs::remove_file(&marker);
        let _ = std::fs::write(&marker, cwd.as_os_str().as_bytes());
    }
}

struct Session {
    cwd: PathBuf,
    since: SystemTime,
}

fn read() -> Option<Session> {
    let marker = marker()?;
    let since = std::fs::metadata(&marker).ok()?.created().ok()?;
    let cwd = PathBuf::from(OsString::from_vec(std::fs::read(&marker).ok()?));
    Some(Session { cwd, since })
}

fn created_since(lane: &Path, since: SystemTime) -> bool {
    std::fs::symlink_metadata(lane).is_ok_and(|metadata| metadata.is_dir() && metadata.created().is_ok_and(|created| created >= since))
}

/// Narrows a fresh domain's `context` to the session that asked for it, so the domain built for one command grants no more than the session's long-lived one did, plus the lanes of the session's project created after that domain was built, whose files its rules could not hold. The working directory must lie outside `~/code`, in the session's project `main`, in its lane, or in such a new lane; the lanes and projects the command's text names count only when they are the session's own or such a new lane. Returns the directories the domain keeps readable: the session's project `main`, its lane, and the new lanes the command names.
pub(super) fn admit(context: &mut Context) -> Result<Vec<PathBuf>, String> {
    let session = read().ok_or("north worker-sandbox: this lane-only session left no marker in its scratch directory, so a fresh domain cannot tell which lanes are its own")?;
    let home = context.home.clone();
    let own_project = project(&home, &session.cwd);
    let own_lane = lane(&home, &session.cwd);
    let admitted = |candidate: &Path| own_lane.as_deref() == Some(candidate) || (own_project.as_ref().is_some_and(|project| candidate.parent() == Some(project.join("worktrees").as_path())) && created_since(candidate, session.since));
    if let Some(project) = project(&home, &context.cwd) {
        let inside = own_project.as_ref() == Some(&project) && lane(&home, &context.cwd).is_none_or(|lane| admitted(&lane));
        if !inside {
            return Err(format!("north worker-sandbox: {} is outside the lane of this session", context.cwd.display()));
        }
    }
    context.named_lanes.retain(|named| admitted(named));
    context.named_projects.retain(|named| own_project.as_ref() == Some(named));
    let mut kept: Vec<PathBuf> = own_project.iter().map(|project| project.join("main")).chain(own_lane).collect();
    kept.extend(context.named_lanes.iter().cloned());
    Ok(kept)
}
