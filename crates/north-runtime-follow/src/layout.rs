//! Where a host keeps its runtimes, which one its links name, and the records a build leaves in one: the tree layout a
//! promotion, a rollback and the collection of an old runtime all read.

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub use north_base::platform::store_refs::RUNTIME_PROGRAMS as PROGRAMS;
pub const BUILT_HERE: &str = "built-here";
pub const BUILT_ON: &str = "built-on";
pub const BUILT_FROM: &str = "built-from";
pub const HELD_ON: &str = "held-on";
pub use north_base::platform::harness::{TREE, is_sha};
use north_base::platform::harness::DEFAULTS;
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

    /// Release runtimes built ahead of the landing they belong to, keyed by the tree hash they were built from, so a promotion that finds its landing's tree here installs it instead of building.
    pub fn artifacts(&self) -> PathBuf {
        self.state.join("runtime-artifacts")
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
        self.state.join(crate::LANDER_ROLLBACK)
    }

    pub fn agent_link_stuck(&self) -> PathBuf {
        self.state.join(north_base::platform::harness::AGENT_LINK_STUCK)
    }

    pub fn swaps(&self) -> PathBuf {
        self.state.join("runtime-swaps.jsonl")
    }

    /// The rollback window the runtime watch last reported, as `SHA SWAPPED_AT`, so it reports each window once.
    pub fn watch_reported(&self) -> PathBuf {
        self.state.join("runtime-watch-reported")
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

/// Whether `artifact` names the build host holding the release north built ahead for its tree and that build's stamp: the programs stay on that host until a promotion pulls them, so a landing copies none here (north#1362).
pub fn held(artifact: &Path) -> bool {
    [HELD_ON, BUILT_FROM].iter().all(|record| artifact.join(record).is_file())
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

/// The runtimes a live process runs from or was launched on: its executable, or a path under `runtimes` in its arguments or environment, where a session's shell prefix and tool server name the runtime it started on.
pub fn running(proc_root: &Path, runtimes: &Path) -> Vec<String> {
    let canonical = fs::canonicalize(runtimes).unwrap_or_else(|_| runtimes.to_path_buf());
    let roots = [canonical, runtimes.to_path_buf()];
    let mut names = Vec::new();
    for process in fs::read_dir(proc_root).into_iter().flatten().flatten() {
        let process = process.path();
        if let Ok(exe) = fs::read_link(process.join("exe")) {
            names.extend(roots.iter().filter_map(|root| exe.strip_prefix(root).ok()?.components().next()?.as_os_str().to_str().map(str::to_owned)));
        }
        for file in ["cmdline", "environ"] {
            if let Ok(text) = fs::read(process.join(file)) {
                names.extend(roots.iter().flat_map(|root| named_under(&text, root)));
            }
        }
    }
    names.retain(|name| is_sha(name));
    names.sort();
    names.dedup();
    names
}

fn named_under(text: &[u8], runtimes: &Path) -> Vec<String> {
    let mut prefix = runtimes.as_os_str().as_bytes().to_vec();
    prefix.push(b'/');
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.windows(prefix.len()).position(|window| window == prefix.as_slice()) {
        rest = &rest[at + prefix.len()..];
        found.extend(rest.get(..40).and_then(|name| std::str::from_utf8(name).ok()).map(str::to_owned));
    }
    found
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
pub fn stamp(runtime: &Path, commit: &str) -> std::io::Result<()> {
    fs::write(runtime.join(BUILT_FROM), format!("{commit} {}\n", binary_hash(&runtime.join("north"))?))
}

/// A runtime built before the record existed is accepted once and stamped when its built-here names its commit, so a change that adds a health requirement can be promoted by the runtime it replaces.
pub fn built_from_own_commit(runtime: &Path) -> Result<(), String> {
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

pub fn binary_hash(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn install(from: &Path, to: &Path) -> std::io::Result<()> {
    let staged = to.with_extension("tmp");
    fs::copy(from, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    fs::rename(&staged, to)
}

