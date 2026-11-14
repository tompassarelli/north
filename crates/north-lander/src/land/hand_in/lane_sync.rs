use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use crate::land::enqueue::git;

fn untracked(root: &Path) -> Result<BTreeSet<String>, String> {
    Ok(git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

/// Whether this process may remove files in the lane, as a rebase that replaces a file must: a worker sandbox lets a process write in a lane its command did not name but not remove there, so a rebase would write the destination's new files, stop at the first file it replaces, and leave them untracked where nothing may remove them. rmdir judges the right to remove from the parent before it looks at the entry, so asking it to remove `.git`, a file or a directory that is never empty, changes nothing.
fn removable(root: &Path) -> bool {
    std::fs::remove_dir(root.join(".git")).is_err_and(|error| matches!(error.kind(), ErrorKind::NotADirectory | ErrorKind::DirectoryNotEmpty))
}

fn forget(root: &Path, path: &str) {
    let mut file = root.join(path);
    let _ = std::fs::remove_file(&file);
    while file.pop() && file != root && std::fs::remove_dir(&file).is_ok() {}
}

fn restore(root: &Path, held: &str, before: &BTreeSet<String>) -> Result<(), String> {
    let _ = git(root, &["rebase", "--abort"]);
    git(root, &["reset", "-q", "--hard", "HEAD"])?;
    for path in untracked(root)?.difference(before) {
        forget(root, path);
    }
    if !held.is_empty() {
        git(root, &["stash", "apply", "-q", "--index", held])?;
    }
    Ok(())
}

pub(super) fn rebase(root: &Path, base: &str, rebased: &str, uncommitted: &str) -> bool {
    if !removable(root) {
        return in_place(root, rebased, uncommitted).inspect_err(|why| eprintln!("north: the lane {} stays on its base: this process may not remove files there, and {why}", root.display())).is_ok();
    }
    let (Ok(before), Ok(held)) = (untracked(root), git(root, &["stash", "create"])) else { return false };
    if git(root, &["rebase", "--quiet", "--autostash", base]).is_ok() {
        return true;
    }
    if let Err(why) = restore(root, &held, &before) {
        eprintln!("north: the lane {} could not be put back as it was after its rebase stopped: {why}", root.display());
    }
    false
}

struct Change {
    path: String,
    before: Option<(String, bool)>,
    after: String,
    executable: bool,
}

fn blob(root: &Path, object: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("git").arg("-C").arg(root).args(["cat-file", "blob", object]).output().map_err(|error| format!("git cat-file: {error}"))?;
    if output.status.success() { Ok(output.stdout) } else { Err(format!("git cat-file blob {object}: {}", String::from_utf8_lossy(&output.stderr).trim())) }
}

fn put(root: &Path, path: &str, object: &str, executable: bool) -> Result<(), String> {
    let file = root.join(path);
    let fail = |error: std::io::Error| format!("{}: {error}", file.display());
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(fail)?;
    }
    std::fs::write(&file, blob(root, object)?).map_err(fail)?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 })).map_err(fail)
}

/// The changes from the lane's working state to `target`, each a regular file written in place or made new: a removal, a change of type and a symbolic link are refused, since only a removal can make them.
fn writable_changes(root: &Path, working: &str, target: &str) -> Result<Vec<Change>, String> {
    let raw = git(root, &["diff-tree", "-r", "-z", "--no-renames", "--raw", working, target])?;
    let mut words = raw.split('\0').filter(|word| !word.is_empty());
    let mut changes = Vec::new();
    while let (Some(header), Some(path)) = (words.next(), words.next()) {
        let fields: Vec<&str> = header.trim_start_matches(':').split(' ').collect();
        let [old_mode, new_mode, old, new, _] = fields[..] else { return Err(format!("git diff-tree printed {header:?}")) };
        let (Ok(old_mode), Ok(new_mode)) = (u32::from_str_radix(old_mode, 8), u32::from_str_radix(new_mode, 8)) else { return Err(format!("git diff-tree printed {header:?}")) };
        let regular = |mode: u32| matches!(mode, 0o100644 | 0o100755);
        let before = match old_mode {
            0 if regular(new_mode) && root.join(path).symlink_metadata().is_err() => None,
            _ if regular(old_mode) && regular(new_mode) => Some((old.to_owned(), old_mode == 0o100755)),
            _ => return Err(format!("moving {path} onto the destination would remove a file")),
        };
        changes.push(Change { path: path.to_owned(), before, after: new.to_owned(), executable: new_mode == 0o100755 });
    }
    Ok(changes)
}

/// Moves a lane onto `rebased` by writing each file the move changes in place and then moving its branch and index, for a lane this process may write but not remove in, where git's rebase replaces a file by removing it first; the uncommitted changes `uncommitted` holds stay uncommitted. A write that fails puts back the files already written that the lane tracked.
fn in_place(root: &Path, rebased: &str, uncommitted: &str) -> Result<(), String> {
    let (working, target) = if uncommitted.is_empty() {
        ("HEAD".to_owned(), rebased.to_owned())
    } else {
        let merged = git(root, &["merge-tree", "--write-tree", "--merge-base", "HEAD", rebased, uncommitted])?;
        (uncommitted.to_owned(), merged.lines().next().unwrap_or_default().to_owned())
    };
    let changes = writable_changes(root, &working, &target)?;
    for (written, change) in changes.iter().enumerate() {
        if let Err(why) = put(root, &change.path, &change.after, change.executable) {
            for undone in &changes[..=written] {
                if let Some((before, executable)) = &undone.before {
                    let _ = put(root, &undone.path, before, *executable);
                }
            }
            return Err(why);
        }
    }
    git(root, &["reset", "-q", rebased]).map(drop)
}
