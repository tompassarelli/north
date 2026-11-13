use super::super::layout::{self, Layout};
use crate::platform::process;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ABANDONED_COPY: Duration = Duration::from_secs(60 * 60);

pub(crate) fn collect(layout: &Layout, target: &str, previous: Option<&Path>) -> std::io::Result<()> {
    collect_beside(layout, target, previous, Path::new("/proc"), &super::agent_link::targets())
}

/// A runtime holding the root of a store path an agent link names stays, so the store never collects the program an agent session runs.
pub(super) fn collect_beside(layout: &Layout, target: &str, previous: Option<&Path>, proc_root: &Path, agent_targets: &[PathBuf]) -> std::io::Result<()> {
    reap_unfinished_copies(&layout.runtimes(), ABANDONED_COPY)?;
    let mut listed = Vec::new();
    for entry in fs::read_dir(layout.runtimes())? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            listed.push((entry.metadata()?.modified()?, entry.file_name().to_string_lossy().into_owned()));
        }
    }
    listed.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let names = listed.into_iter().map(|(_, name)| name).collect::<Vec<_>>();
    let mut keep = vec![target];
    keep.extend(previous.and_then(layout::linked_sha));
    let lander = fs::read_link(layout.lander()).ok();
    keep.extend(lander.as_deref().and_then(layout::linked_sha));
    let rollback = fs::read_to_string(layout.lander_rollback()).ok();
    keep.extend(rollback.as_deref().and_then(|target| layout::linked_sha(Path::new(target.trim()))));
    let running = layout::running(proc_root, &layout.runtimes());
    keep.extend(running.iter().map(String::as_str));
    keep.extend(names.iter().map(String::as_str).filter(|name| roots_any(&layout.runtime(name), agent_targets)));
    let current = fs::canonicalize(layout.current()).ok();
    for name in layout::doomed(&names, &keep) {
        let doomed = layout.runtime(name);
        if fs::canonicalize(&doomed).ok() == current {
            continue;
        }
        let Some(_unleased) = layout::unleased(&doomed)? else {
            continue;
        };
        let _ = fs::remove_file(layout.state.join(layout::writing_lock(name)));
        process::run("chmod", None, &[OsStr::new("-R"), "u+w".as_ref(), doomed.as_os_str()])?;
        fs::remove_dir_all(&doomed)?;
    }
    Ok(())
}

/// A copy shipped over a slow link stages in `SHA.staging` and moves into place only whole, so one that stopped part-way leaves its staging directory behind; one untouched for `idle` has no copy writing to it.
fn reap_unfinished_copies(runtimes: &Path, idle: Duration) -> std::io::Result<()> {
    for entry in fs::read_dir(runtimes)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let unfinished = [".staging", ".old"].iter().any(|suffix| name.strip_suffix(suffix).is_some_and(layout::is_sha));
        if !unfinished || !entry.file_type()?.is_dir() || entry.metadata()?.modified()?.elapsed().unwrap_or_default() < idle {
            continue;
        }
        process::run("chmod", None, &[OsStr::new("-R"), "u+w".as_ref(), entry.path().as_os_str()])?;
        fs::remove_dir_all(entry.path())?;
    }
    Ok(())
}

fn roots_any(runtime: &Path, targets: &[PathBuf]) -> bool {
    !targets.is_empty() && fs::read_dir(runtime).into_iter().flatten().flatten().any(|entry| fs::read_link(entry.path()).is_ok_and(|rooted| targets.contains(&rooted)))
}
