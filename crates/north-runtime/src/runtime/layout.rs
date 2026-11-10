use std::fs;use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub use crate::platform::store_refs::RUNTIME_PROGRAMS as PROGRAMS;
pub const BUILT_HERE: &str = "built-here";
pub const BUILT_ON: &str = "built-on";
pub const BUILT_FROM: &str = "built-from";
pub use crate::platform::harness::{TREE, is_sha};
use crate::platform::harness::DEFAULTS;
use rustix::fs::{FlockOperation, flock};

pub const LEASE: &str = "lease";
const KEPT_OLDER: usize = 3;

pub struct Layout {
    pub state: PathBuf,
}

impl Layout {
    pub fn from_env() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/state"));
        Self { state: state.join("north") }
    }

    pub fn runtimes(&self) -> PathBuf {
        self.state.join("runtimes")
    }

    pub fn current(&self) -> PathBuf {
        self.state.join("current")
    }

    pub fn ledger(&self) -> PathBuf {
        self.state.join("ledger.db")
    }

    pub fn live_sha(&self) -> Option<String> {
        std::fs::read_link(self.current()).ok().and_then(|link| linked_sha(&link).map(str::to_owned))
    }

    pub fn lander(&self) -> PathBuf {
        self.state.join("lander")
    }

    pub fn lander_rollback(&self) -> PathBuf {
        self.state.join(north_runtime_follow::LANDER_ROLLBACK)
    }

    pub fn swaps(&self) -> PathBuf {
        self.state.join("runtime-swaps.jsonl")
    }

    pub fn build_logs(&self) -> PathBuf {
        self.state.join("runtime-build-logs")
    }

    pub fn runtime(&self, sha: &str) -> PathBuf {
        self.runtimes().join(sha)
    }
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn writing_lock(sha: &str) -> String {
    format!("runtimes/{sha}.lock")
}

pub fn short(sha: &str) -> &str {
    sha.get(..8).unwrap_or(sha)
}

pub fn link(sha: &str) -> PathBuf {
    Path::new("runtimes").join(sha)
}

pub fn linked_sha(link: &Path) -> Option<&str> {
    link.strip_prefix("runtimes").ok()?.to_str().filter(|name| is_sha(name))
}

pub fn has_programs(runtime: &Path) -> bool {
    PROGRAMS.iter().all(|program| runtime.join(program).is_file())
}

pub fn usable(runtime: &Path, commissioned_only: bool) -> bool {
    has_programs(runtime) && (!commissioned_only || commissioned(runtime))
}

/// Whether this host built the runtime or had a build host build it for it, as against one a peer shipped or left here.
pub fn commissioned(runtime: &Path) -> bool {
    [BUILT_HERE, BUILT_ON].iter().any(|marker| runtime.join(marker).is_file())
}

pub fn tree_marker() -> String {
    format!("{TREE}/{DEFAULTS}")
}

pub fn unleased(runtime: &Path) -> std::io::Result<Option<fs::File>> {
    let lease = fs::OpenOptions::new().create(true).append(true).open(runtime.join(LEASE))?;
    Ok(flock(&lease, FlockOperation::NonBlockingLockExclusive).is_ok().then_some(lease))
}

pub fn running(proc_root: &Path, runtimes: &Path) -> Vec<String> {
    let runtimes = fs::canonicalize(runtimes).unwrap_or_else(|_| runtimes.to_path_buf());
    let mut names = fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|process| fs::read_link(process.path().join("exe")).ok())
        .filter_map(|exe| exe.strip_prefix(&runtimes).ok()?.components().next()?.as_os_str().to_str().map(str::to_owned))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

pub fn doomed<'a>(newest_first: &'a [String], keep: &[&str]) -> Vec<&'a str> {
    newest_first
        .iter()
        .map(String::as_str)
        .filter(|name| is_sha(name) && !keep.contains(name))
        .skip(KEPT_OLDER)
        .collect()
}

/// Records the commit a runtime's binary was built for beside the binary's hash, written only by a build whose cargo ran or a reuse that proved no Rust input changed.
pub(super) fn stamp(runtime: &Path, commit: &str) -> std::io::Result<()> {
    fs::write(runtime.join(BUILT_FROM), format!("{commit} {}\n", binary_hash(&runtime.join("north"))?))
}

/// A runtime built before the record existed is accepted once and stamped when its built-here names its commit, so a change that adds a health requirement can be promoted by the runtime it replaces.
pub(super) fn built_from_own_commit(runtime: &Path) -> Result<(), String> {
    let commit = runtime.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let stamp = match fs::read_to_string(runtime.join(BUILT_FROM)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && fs::read_to_string(runtime.join(BUILT_HERE)).is_ok_and(|here| here.trim() == short(&commit)) => {
            stamp(runtime, &commit).map_err(|error| format!("{}: {error}", runtime.display()))?;
            return Ok(());
        }
        read => read.map_err(|error| format!("{} has no {BUILT_FROM} record ({error})", runtime.display()))?,
    };
    let hash = binary_hash(&runtime.join("north")).map_err(|error| format!("{}: {error}", runtime.join("north").display()))?;
    match stamp.split_whitespace().collect::<Vec<_>>()[..] {
        [stamped, recorded] if stamped == commit && recorded == hash => Ok(()),
        _ => Err(format!("the north binary in {} was not built from its own commit {commit}", runtime.display())),
    }
}

pub(super) fn binary_hash(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn install(from: &Path, to: &Path) -> std::io::Result<()> {
    let staged = to.with_extension("tmp");
    fs::copy(from, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    fs::rename(&staged, to)
}

