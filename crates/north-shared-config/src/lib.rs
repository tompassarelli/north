//! Shared configuration a host reads: the top-level files of south's `agents/` at a fetched origin/main, materialized
//! under the state directory, never the south checkout's work tree, which lags (north#892).
//! Depends only on std and the git binary.
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The longest a host may run on shared configuration after its last successful fetch of south's origin/main.
pub const BOUND_SECONDS: u64 = 60 * 60;
/// How long the runtime update tick waits between fetches of south's origin/main.
pub const REFRESH_SECONDS: u64 = 5 * 60;
const SOURCE: &str = "origin/main";
const TREE: &str = "agents";
const STAMP: &str = "stamp";
const CURRENT: &str = "current";

/// Seconds since the Unix epoch, the clock every stamp and bound here is measured on.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

/// The fetched revision a host reads its shared configuration from, and when that fetch succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub revision: String,
    pub fetched_at: u64,
}

pub fn directory(home: &Path) -> PathBuf {
    home.join(".local/state/north/shared-config")
}

/// The snapshot of south's `agents/` top-level files at the fetched revision, the directory every shared configuration file is read from.
pub fn current(home: &Path) -> PathBuf {
    directory(home).join(CURRENT)
}

pub fn south(home: &Path) -> PathBuf {
    home.join("code/south/main")
}

/// The orchestration file a run reads: the one `orchestration` names, else the operator directory's, else the
/// snapshot's, else the south checkout's; every other policy file is read beside it, at the same revision.
pub fn orchestration(home: &Path, orchestration: Option<&Path>, operator: Option<&Path>) -> PathBuf {
    const FILE: &str = "orchestration.toml";
    if let Some(path) = orchestration {
        return path.to_owned();
    }
    if let Some(operator) = operator {
        return operator.join(FILE);
    }
    let snapshot = current(home).join(FILE);
    match snapshot.is_file() {
        true => snapshot,
        false => south(home).join(TREE).join(FILE),
    }
}

pub fn stamp(home: &Path) -> Option<Stamp> {
    let text = fs::read_to_string(directory(home).join(STAMP)).ok()?;
    let (revision, fetched_at) = text.trim().split_once(' ')?;
    Some(Stamp { revision: revision.to_owned(), fetched_at: fetched_at.parse().ok()? })
}

/// The shared configuration's stamp when its last fetch is within the bound, else why this host must not run on it.
pub fn fresh(home: &Path, now: u64) -> Result<Stamp, String> {
    let Some(stamp) = stamp(home).filter(|_| current(home).is_dir()) else {
        return Err(format!("shared configuration: no fetched snapshot at {}; run north runtime update", current(home).display()));
    };
    let age = now.saturating_sub(stamp.fetched_at);
    if age > BOUND_SECONDS {
        return Err(format!(
            "shared configuration: last fetch of south {SOURCE} ({}) was {} min ago, past the {} min bound; refusing to run on it until a fetch succeeds",
            &stamp.revision[..stamp.revision.len().min(10)],
            age / 60,
            BOUND_SECONDS / 60
        ));
    }
    Ok(stamp)
}

/// Fetch south's origin/main into its object store and materialize `agents/`'s top-level files at that revision, never reading the checkout's work tree.
pub fn refresh(home: &Path, repo: &Path, now: u64) -> Result<Stamp, String> {
    let ran = |args: &[&str]| -> Result<Vec<u8>, String> {
        let output = Command::new("git").arg("-C").arg(repo).args(args).output().map_err(|error| format!("git {}: {error}", args.join(" ")))?;
        match output.status.success() {
            true => Ok(output.stdout),
            false => Err(format!("git {} in {}: {}", args.join(" "), repo.display(), String::from_utf8_lossy(&output.stderr).trim())),
        }
    };
    ran(&["fetch", "-q", "origin", "main"])?;
    let revision = String::from_utf8_lossy(&ran(&["rev-parse", "--verify", SOURCE])?).trim().to_owned();
    let root = directory(home);
    let snapshot = root.join(&revision);
    if !snapshot.is_dir() {
        let listing = ran(&["ls-tree", "-z", &revision, &format!("{TREE}/")])?;
        let staged = root.join(format!("{revision}.staged"));
        let _ = fs::remove_dir_all(&staged);
        fs::create_dir_all(&staged).map_err(|error| format!("{}: {error}", staged.display()))?;
        for entry in listing.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
            let entry = String::from_utf8_lossy(entry);
            let Some((mode, path)) = entry.split_once('\t') else { continue };
            if !mode.contains(" blob ") {
                continue;
            }
            let name = path.strip_prefix(&format!("{TREE}/")).unwrap_or(path);
            let body = ran(&["cat-file", "blob", &format!("{revision}:{path}")])?;
            fs::write(staged.join(name), body).map_err(|error| format!("{name}: {error}"))?;
        }
        fs::rename(&staged, &snapshot).map_err(|error| format!("{}: {error}", snapshot.display()))?;
    }
    point(&root, &revision)?;
    let stamp = Stamp { revision, fetched_at: now };
    let pending = root.join(format!("{STAMP}.pending"));
    fs::write(&pending, format!("{} {}\n", stamp.revision, stamp.fetched_at)).map_err(|error| error.to_string())?;
    fs::rename(&pending, root.join(STAMP)).map_err(|error| error.to_string())?;
    prune(&root, &stamp.revision);
    Ok(stamp)
}

/// Refresh when the last fetch is older than the refresh interval; a failed fetch keeps the snapshot, which `fresh` bounds.
pub fn refresh_if_due(home: &Path, now: u64) -> Option<Result<Stamp, String>> {
    let due = stamp(home).is_none_or(|stamp| now.saturating_sub(stamp.fetched_at) >= REFRESH_SECONDS);
    (due && south(home).is_dir()).then(|| refresh(home, &south(home), now))
}

fn point(root: &Path, revision: &str) -> Result<(), String> {
    let link = root.join(CURRENT);
    if fs::read_link(&link).is_ok_and(|target| target == Path::new(revision)) {
        return Ok(());
    }
    let staged = root.join(format!("{CURRENT}.pending"));
    let _ = fs::remove_file(&staged);
    std::os::unix::fs::symlink(revision, &staged).map_err(|error| error.to_string())?;
    fs::rename(&staged, &link).map_err(|error| error.to_string())
}

fn prune(root: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let revision = name.strip_suffix(".staged").unwrap_or(&name);
        if name != keep && revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests;
