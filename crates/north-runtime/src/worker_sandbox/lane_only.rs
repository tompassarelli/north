use std::path::{Path, PathBuf};

use super::Context;
use super::except::{Entries, beneath_except};
use super::readable::personal_logins;
use super::removable::{lane, project};

/// Home entries an agent confined to its lane still reads: the toolchains, git's own configuration and `.local`, of which only `bin`, `share/north` and the `lib/opencode` and `lib/pi` programs the DeepSeek harnesses run stay readable, the places the toolchain shims and those programs live, and the north runtimes directory wherever the host keeps it: `cargo` and `north` on `PATH` resolve through them, and a lane's tests and builds run only through those shims, which enter depbox (south#93).
const TOOLCHAINS: [&str; 6] = [".cargo", ".rustup", ".bun", ".npm", ".gitconfig", ".local"];
pub(super) const SCRATCH_ENV: &str = "NORTH_SANDBOX_SCRATCH";

/// Every file a command confined to its lane may read: all of them but the operator's logins, every other checkout, lane and project, the agent state under `~/.local` apart from the north runtimes, the rest of `~/.config`, and other sessions' scratch under `/tmp`, `/var/tmp` and the runtime directory. What stays readable is the lane, its project's `main` checkout (git's common directory), the toolchains, the north runtimes, the system, and the `fresh` directories of a fresh domain (lane_session.rs).
pub fn readable_in_lane(context: &Context, fresh: &[PathBuf]) -> Vec<PathBuf> {
    beneath_except(Path::new("/"), &hidden(context, fresh), Entries::All)
}

pub(super) fn hidden(context: &Context, fresh: &[PathBuf]) -> Vec<PathBuf> {
    let home = &context.home;
    let code = home.join("code");
    let mut hidden = personal_logins(context);
    let kept: Vec<PathBuf> = TOOLCHAINS.iter().map(|name| home.join(name)).chain([code.clone(), home.join(".config")]).collect();
    hidden.extend(others(home, &kept));
    hidden.extend(others(&home.join(".config"), &[home.join(".config/git")]));
    let local = home.join(".local");
    hidden.extend(others(&local, &[local.join("bin"), local.join("share"), local.join("lib")]));
    hidden.extend(others(&local.join("lib"), &[local.join("lib/opencode"), local.join("lib/pi")]));
    hidden.extend(others(&local.join("share"), &[local.join("share/north")]));
    match project(home, &context.cwd) {
        Some(project) => {
            hidden.extend(others(&code, std::slice::from_ref(&project)));
            hidden.extend(others(&project, &[project.join("main"), project.join("worktrees")]));
            hidden.extend(others(&project.join("worktrees"), &lane(home, &context.cwd).into_iter().collect::<Vec<_>>()));
        }
        None => hidden.extend(others(&code, &[])),
    }
    let scratch: Vec<PathBuf> = std::env::var_os(SCRATCH_ENV).map(PathBuf::from).into_iter().collect();
    for shared in ["/tmp", "/var/tmp"] {
        hidden.extend(others(Path::new(shared), &scratch_under(Path::new(shared), &scratch)));
    }
    hidden.extend(others(Path::new(&format!("/run/user/{}", context.uid)), &[]));
    if let Some(runtimes) = &context.runtimes {
        reveal(&mut hidden, runtimes);
    }
    for directory in fresh {
        reveal(&mut hidden, directory);
    }
    hidden
}

/// Narrows the hidden entry that holds `kept` to its other entries at each level down to `kept`, so the north runtimes stay readable while the agent state beside them stays hidden.
fn reveal(hidden: &mut Vec<PathBuf>, kept: &Path) {
    let holding: Vec<PathBuf> = hidden.iter().map(|path| real(path)).filter(|path| kept.starts_with(path)).collect();
    hidden.retain(|path| !touches(&real(path), kept));
    for directory in holding {
        descend(hidden, directory, kept);
    }
}

fn real(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn touches(path: &Path, kept: &Path) -> bool {
    kept.starts_with(path) || path.starts_with(kept)
}

fn descend(hidden: &mut Vec<PathBuf>, mut directory: PathBuf, kept: &Path) {
    let below: Vec<PathBuf> = kept.strip_prefix(&directory).map(|rest| rest.components().map(|part| PathBuf::from(part.as_os_str())).collect()).unwrap_or_default();
    for part in below {
        let next = directory.join(part);
        hidden.extend(others(&directory, std::slice::from_ref(&next)).into_iter().filter(|path| !touches(&real(path), kept)));
        directory = next;
    }
}

fn scratch_under(shared: &Path, scratch: &[PathBuf]) -> Vec<PathBuf> {
    scratch.iter().filter_map(|path| path.strip_prefix(shared).ok()?.components().next().map(|first| shared.join(first))).collect()
}

fn others(directory: &Path, kept: &[PathBuf]) -> Vec<PathBuf> {
    std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| !kept.contains(path)).collect()
}
