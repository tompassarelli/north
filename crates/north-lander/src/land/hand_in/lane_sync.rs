use std::collections::BTreeSet;
use std::path::Path;

use crate::land::enqueue::git;

fn untracked(root: &Path) -> Result<BTreeSet<String>, String> {
    Ok(git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
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
    let (Ok(before), Ok(held)) = (untracked(root), git(root, &["stash", "create"])) else { return false };
    if git(root, &["rebase", "--quiet", "--autostash", base]).is_ok() {
        return true;
    }
    if let Err(why) = restore(root, &held, &before) {
        eprintln!("north: the lane {} could not be put back as it was after its rebase stopped: {why}", root.display());
    }
    false
}
