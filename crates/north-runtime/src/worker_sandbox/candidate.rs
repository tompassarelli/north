//! `north --worker-sandbox --candidate -- PROGRAM [ARGS...]`: runs one land check of a lander's candidate in the checkout it runs in, under a bubblewrap mount namespace laid out as depbox lays a build's: a read-only root, an empty home directory and secrets directory in place of the real ones and every other path home shows up at, depbox's read-only tools and the live runtime, and as writable only that checkout (its `.git` and git directories excepted), a private `/tmp` and runtime directory, the package and build caches depbox lets a build write and depbox's fetch records. The check runs the lane's own code, so it cannot read the logins or write the project's trusted mirror beside the checkout, a main checkout, another lane or the agent state, nor reach the user's service manager, the build cache server or another process of the user (south#138). It is a mount namespace rather than a Landlock domain because the domain refuses mount(2) to every process in it, and a land check enters depbox and runs tests that build namespaces of their own, which nest inside this one.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use north_capacity_host::capacity::depbox;
use crate::runtime::layout;
use crate::runtime::update::pull_held;

use super::removable::git_dirs;

/// The records under the home directory a land check writes: the lock files depbox keeps of the crates it fetched, which only spare a later fetch.
const WRITTEN_STATE: [&str; 1] = [".local/state/depbox"];
/// What a land check reads under the home directory beyond depbox's reads: the north runtimes build-ahead made for a tree, which `bin/head-north` runs, and the review accepts and finish records the review-accept and safe-push-keys scans count, here and as the records mirror carries them from the settling host with the stamp that says it does.
const READ_STATE: [&str; 5] = [
    ".local/state/north/runtime-artifacts",
    ".local/state/north/verdict-cache/accepts",
    ".local/state/north/verdict-cache/finished",
    ".local/state/north/records-mirror/north/verdict-cache/accepts",
    ".local/state/north/records-mirror/north/records-stamp.json",
];
const SECRETS: [&str; 2] = ["/run/secrets", "/run/secrets.d"];
const LIVE: &str = ".local/state/north/current";
/// The lease records a held check's depbox reads through `capacity deadline`, under the runtime directory the jail hides.
const LEASES: &str = "agent-capacity-v1/leases";
/// The pinned checkouts of other projects an offloaded check reads beside its own (north-offload ship/beside.rs), which its check lines name instead of `~/code/PROJECT/main`.
const BESIDE: &str = "NORTH_OFFLOAD_BESIDE";

/// Where a candidate's land check runs and what it may write.
pub struct Candidate {
    pub home: PathBuf,
    pub checkout: PathBuf,
    pub runtime: PathBuf,
    pub scratch: PathBuf,
}

impl Candidate {
    /// The bubblewrap words, before `--` and the program, that confine a land check to this candidate.
    pub fn arguments(&self) -> Vec<OsString> {
        let mut words = Words::default();
        words.add(&["--ro-bind", "/", "/", "--dev", "/dev", "--unshare-pid", "--unshare-ipc", "--proc", "/proc", "--die-with-parent", "--new-session"]);
        let named = std::env::var_os("HOME").map(PathBuf::from).filter(|named| named.is_absolute() && *named != self.home);
        let brokers = [north_base::platform::broker_client::SYSTEM_SOCKET, north_base::platform::broker_client::AGENT_SOCKET].into_iter().filter_map(|socket| Path::new(socket).parent());
        let secrets = SECRETS.iter().map(PathBuf::from).chain(brokers.map(Path::to_path_buf)).filter(|directory| std::fs::symlink_metadata(directory).is_ok_and(|metadata| metadata.is_dir()));
        for hidden in std::iter::once(self.home.clone()).chain(named).chain(depbox::aliases(&self.home)).chain(secrets).chain(Some(self.runtime.clone()).filter(|directory| directory.is_dir())) {
            words.add(&["--tmpfs"]);
            words.0.push(hidden.into());
        }
        words.bind("--bind", &self.scratch, Path::new("/tmp"));
        words.add(&["--setenv", "TMPDIR", "/tmp", "--unsetenv", "SCCACHE_SERVER_PORT", "--setenv", "SCCACHE_SERVER_UDS", "/tmp/sccache.sock"]);
        for read in depbox::READS.iter().chain(&READ_STATE).map(|path| self.home.join(path)).filter(|path| path.exists()) {
            let real = depbox::resolved(&read);
            words.bind("--ro-bind", &read, &read);
            if real != read {
                words.bind("--ro-bind", &real, &real);
            }
        }
        let leases = self.runtime.join(LEASES);
        if leases.is_dir() {
            words.bind("--ro-bind", &leases, &leases);
        }
        let live = self.home.join(LIVE);
        if let Ok(target) = std::fs::read_link(&live) {
            words.bind("--symlink", &target, &live);
        }
        for written in depbox::CACHES.iter().chain(&depbox::BUILD_CACHES).chain(&WRITTEN_STATE).map(|path| self.home.join(path)) {
            words.bind("--bind", &written, &written);
        }
        words.bind("--bind", &self.checkout, &self.checkout);
        let dot = self.checkout.join(".git");
        if dot.exists() {
            words.bind("--ro-bind", &dot, &dot);
        }
        let beside = std::env::var_os(BESIDE).map(PathBuf::from).filter(|beside| beside.is_absolute() && beside.is_dir());
        let besides: Vec<PathBuf> = beside.iter().flat_map(|beside| std::fs::read_dir(beside).into_iter().flatten().flatten().map(|entry| entry.path())).collect();
        for read in beside.iter().cloned().chain(besides.iter().filter_map(|entry| self.main_git_dirs(entry)).flat_map(|(git_dir, common)| [common, git_dir])) {
            words.bind("--ro-bind", &read, &read);
        }
        if let Some((git_dir, common)) = git_dirs(&self.checkout) {
            for directory in [common, git_dir] {
                words.bind("--ro-bind", &directory, &directory);
            }
        }
        words.add(&["--chdir"]);
        words.0.push(self.checkout.clone().into());
        words.0
    }

    /// The git directories of the beside checkout `entry`, only when they lie in the repository of `~/code/PROJECT/main` its name names, which the slot script made it a worktree of: the beside directory is one a worker writes, so a `.git` it planted pointing anywhere else binds nothing.
    fn main_git_dirs(&self, entry: &Path) -> Option<(PathBuf, PathBuf)> {
        let project = entry.file_name()?.to_str().filter(|name| !name.is_empty() && !name.starts_with('.'))?;
        let repository = self.home.join("code").join(project).join("main/.git").canonicalize().ok()?;
        let (git_dir, common) = git_dirs(entry)?;
        let (git_dir, common) = (git_dir.canonicalize().ok()?, common.canonicalize().ok()?);
        (common == repository && git_dir.starts_with(&repository)).then_some((git_dir, common))
    }
}
#[derive(Default)]
struct Words(Vec<OsString>);

impl Words {
    fn add(&mut self, words: &[&str]) {
        self.0.extend(words.iter().map(OsString::from));
    }

    fn bind(&mut self, flag: &str, source: &Path, destination: &Path) {
        self.0.extend([OsString::from(flag), source.into(), destination.into()]);
    }
}

/// Makes the caches and records a land check writes where missing, since a bind needs its source, and returns the candidate this process's working directory is.
fn here(scratch: &Path) -> Result<Candidate, String> {
    let named = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or("HOME is not set")?;
    let home = std::fs::canonicalize(&named).map_err(|error| format!("HOME {}: {error}", named.to_string_lossy()))?;
    let checkout = std::env::current_dir().and_then(std::fs::canonicalize).map_err(|error| format!("the working directory: {error}"))?;
    if checkout == home || checkout == Path::new("/") {
        return Err(format!("refusing to leave {} writable", checkout.display()));
    }
    for written in depbox::CACHES.iter().chain(&depbox::BUILD_CACHES).chain(&WRITTEN_STATE).map(|path| home.join(path)) {
        std::fs::create_dir_all(&written).map_err(|error| format!("{}: {error}", written.display()))?;
    }
    let _ = std::fs::create_dir_all(home.join(depbox::TRUSTED_BUILDS));
    prepared(&home.join(".local/state/north"), &checkout);
    let runtime = PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw()));
    Ok(Candidate { home, checkout, runtime, scratch: scratch.to_owned() })
}

/// Leaves on this host, before the check is confined, what it needs under the north state it reads only: a lease file in every runtime, which `bin/head-north` takes shared before it runs one and cannot create through the read-only bind, and the north built ahead for the checkout's tree, pulled from the build host that holds it over the ssh the confinement hides (north#1578).
fn prepared(state: &Path, checkout: &Path) {
    for runtime in std::fs::read_dir(state.join("runtimes")).into_iter().flatten().flatten().map(|entry| entry.path()) {
        if runtime.file_name().and_then(|name| name.to_str()).is_some_and(layout::is_sha) && runtime.is_dir() {
            let _ = std::fs::OpenOptions::new().create(true).append(true).open(runtime.join(layout::LEASE));
        }
    }
    let Ok(output) = Command::new("git").arg("-C").arg(checkout).args(["rev-parse", "HEAD^{tree}"]).output() else { return };
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let artifact = state.join("runtime-artifacts").join(&tree);
    if !layout::is_sha(&tree) || !layout::held(&artifact) || artifact.join("north").is_file() {
        return;
    }
    let Ok(lock) = std::fs::OpenOptions::new().create(true).append(true).open(artifact.join("pull.lock")) else { return };
    if rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive).is_ok() && !artifact.join("north").is_file() && let Err(failure) = pull_held(&artifact, &tree) {
        eprintln!("north worker-sandbox: {}", failure.message);
    }
}

/// Runs `program` confined to the candidate checkout of the working directory, in a scratch directory of its own that is removed when it ends; its exit code.
pub(super) fn run(program: &[String]) -> ExitCode {
    let scratch = match tempfile::Builder::new().prefix("north-candidate-").tempdir() {
        Ok(scratch) => scratch,
        Err(error) => {
            eprintln!("north worker-sandbox: a candidate check's scratch directory: {error}");
            return ExitCode::from(126);
        }
    };
    let candidate = match here(scratch.path()) {
        Ok(candidate) => candidate,
        Err(why) => {
            eprintln!("north worker-sandbox: a candidate check did not run: {why}");
            return ExitCode::from(126);
        }
    };
    let status = Command::new("bwrap").args(candidate.arguments()).arg("--").args(program).status();
    match status {
        Ok(status) => ExitCode::from(exit_code(status)),
        Err(error) => {
            eprintln!("north worker-sandbox: bwrap, which confines a candidate check: {error}");
            ExitCode::from(126)
        }
    }
}

fn exit_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    let code = status.code().or_else(|| status.signal().map(|signal| 128 + signal)).unwrap_or(1);
    u8::try_from(code & 0xff).unwrap_or(1)
}
