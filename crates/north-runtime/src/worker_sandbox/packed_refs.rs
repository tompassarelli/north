//! Where a worker's git rewrites `packed-refs`. git deletes a ref, absent ones included (`CHERRY_PICK_HEAD` on every commit), by creating `packed-refs.lock` beside `packed-refs` and renaming it over the file, at the top of the common directory, where `config` is replaced the same way and a Landlock right on the directory reaches both. git creates the lock beside the file a symlink names, so `packed-refs` links into `packed`, a directory whose regular files a worker may write, make and replace and where it may make nothing else, so neither a link out nor a subdirectory can be planted for a git outside the domain to follow.
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use super::Context;
use super::removable::{PACKED as DIRECTORY, project_git_common_dirs};

const FILE: &str = "packed-refs";

/// The `packed` directory of the git common directory of each project a command run from `context` works in, as its main checkout names it, laid out first where `packed-refs` does not yet link into it: a lane shares its project's, and one whose gitfile names another gets none.
pub(super) fn directories(context: &Context) -> Vec<PathBuf> {
    project_git_common_dirs(context).iter().filter_map(|common| laid_out(common)).collect()
}

/// `common`'s `packed` directory once its `packed-refs` links into it. The layout changes under git's own `packed-refs.lock`, so no git rewrites the file meanwhile, and the file is linked into `packed` before a symlink is renamed over it, so a reader finds every packed ref throughout.
fn laid_out(common: &Path) -> Option<PathBuf> {
    let directory = common.join(DIRECTORY);
    let packed = common.join(FILE);
    let links_in = || fs::read_link(&packed).is_ok_and(|target| target == Path::new(DIRECTORY).join(FILE)) && directory.is_dir();
    if links_in() {
        return Some(directory);
    }
    if packed.is_symlink() {
        return None;
    }
    let lock = common.join(format!("{FILE}.lock"));
    if fs::create_dir_all(&directory).is_err() || OpenOptions::new().write(true).create_new(true).open(&lock).is_err() {
        return links_in().then_some(directory);
    }
    let linked = link_in(common, &packed, &directory);
    let _ = fs::remove_file(&lock);
    linked.ok().filter(|()| links_in()).map(|()| directory)
}

fn link_in(common: &Path, packed: &Path, directory: &Path) -> io::Result<()> {
    let inside = directory.join(FILE);
    absent_ok(fs::remove_file(&inside))?;
    absent_ok(fs::hard_link(packed, &inside))?;
    let staged = common.join(format!("{FILE}.north"));
    absent_ok(fs::remove_file(&staged))?;
    std::os::unix::fs::symlink(Path::new(DIRECTORY).join(FILE), &staged)?;
    fs::rename(&staged, packed)
}

fn absent_ok(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
