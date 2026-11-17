//! Deleting the runtimes no link, lease, live process or agent link needs: the collection a promotion runs after its
//! swap, over this host's process table and the store paths its agent links name.

use crate::layout::{self, Layout};
use north_base::platform::process;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ABANDONED_COPY: Duration = Duration::from_secs(60 * 60);

/// One runtime holding the root of each store path an agent link names stays, so the store never collects the program an agent session runs; every runtime built since that agent roots it too, so keeping all of them kept every runtime (north#1054).
pub fn collect(layout: &Layout, target: &str, previous: Option<&Path>, proc_root: &Path, agent_targets: &[PathBuf]) -> std::io::Result<()> {
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
    let running = layout::running(proc_root, &layout.runtimes());
    keep.extend(running.iter().map(String::as_str));
    for target in agent_targets {
        let roots = |name: &&str| roots_any(&layout.runtime(name), std::slice::from_ref(target));
        if !keep.iter().any(roots) {
            keep.extend(names.iter().map(String::as_str).filter(|name| layout::is_sha(name)).find(roots));
        }
    }
    let current = fs::canonicalize(layout.current()).ok();
    for name in layout::doomed(&names, &keep) {
        remove(layout, name, current.as_deref())?;
    }
    Ok(())
}

/// The sweep the laptop's disk pass runs between promotions: every runtime past the newest `newest` that no link, live process or lease needs; the names it deleted.
pub fn prune(layout: &Layout, proc_root: &Path, newest: usize) -> std::io::Result<Vec<String>> {
    reap_unfinished_copies(&layout.runtimes(), ABANDONED_COPY)?;
    let mut listed = Vec::new();
    for entry in fs::read_dir(layout.runtimes())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() && layout::is_sha(&name) {
            listed.push((entry.metadata()?.modified()?, name));
        }
    }
    listed.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let mut keep = Vec::new();
    for link in [fs::read_link(layout.current()).ok(), fs::read_link(layout.lander()).ok()] {
        keep.extend(link.as_deref().and_then(layout::linked_sha).map(str::to_owned));
    }
    keep.extend(layout::running(proc_root, &layout.runtimes()));
    let current = fs::canonicalize(layout.current()).ok();
    let mut removed = Vec::new();
    for (_, name) in listed.into_iter().skip(newest) {
        if !keep.contains(&name) && remove(layout, &name, current.as_deref())? {
            removed.push(name);
        }
    }
    Ok(removed)
}

/// Deletes the runtime `name` unless it is the current one, a lease holds it, or it resolves outside the runtimes store; whether it was deleted.
fn remove(layout: &Layout, name: &str, current: Option<&Path>) -> std::io::Result<bool> {
    let doomed = layout.runtime(name);
    let resolved = fs::canonicalize(&doomed)?;
    if Some(resolved.as_path()) == current || !layout::is_sha(name) || resolved != fs::canonicalize(layout.runtimes())?.join(name) {
        return Ok(false);
    }
    let Some(_unleased) = layout::unleased(&doomed)? else {
        return Ok(false);
    };
    let _ = fs::remove_file(layout.state.join(layout::writing_lock(name)));
    process::run("chmod", None, &[OsStr::new("-R"), "u+w".as_ref(), doomed.as_os_str()])?;
    fs::remove_dir_all(&doomed)?;
    Ok(true)
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
