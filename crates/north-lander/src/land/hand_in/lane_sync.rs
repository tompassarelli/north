use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::Path;

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

pub(super) fn rebase(root: &Path, base: &str) -> bool {
    if !removable(root) {
        eprintln!("north: the lane {} stays on its base: this process may not remove files there, so a rebase would stop and leave the destination's new files behind", root.display());
        return false;
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
