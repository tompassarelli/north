//! `north capacity sandbox --lane DIR -- COMMAND...`: runs a command another host composed, such as a heavy run pulled from a peer's queue, inside bubblewrap so its build scripts, tests and dev-shell hook never touch this host's credentials. The lane is the only writable checkout; HOME and TMPDIR are private and empty; the nix store, the cargo registry, the compiler cache and the lane's git directory are read-only; the network is a fresh namespace with only its own loopback, so the command reaches no host at all and the nix daemon socket is its one way out of the sandbox.
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::build_offload::{REMOTE_MARK, SHIP_SECONDS_ENV, SLOT_HEAD_ENV, SLOT_TARGET_ENV};
use super::dev_shell;

const USAGE: &str = "usage: north capacity sandbox --lane DIR -- COMMAND...";
const SYSTEM: [&str; 7] = ["/nix", "/etc", "/run/current-system", "/bin", "/usr", "/lib64", "/sys"];
const PASSED: [&str; 18] = [
    "PATH", "LANG", "LC_ALL", "TERM", "USER", "LOGNAME", "CARGO_BUILD_JOBS", "RUST_TEST_THREADS", "AGENT_CAPACITY_LEASE", "AGENT_CAPACITY_PAIR", "RUSTC_WRAPPER", "SCCACHE_DIR", "SCCACHE_CACHE_SIZE", "SCCACHE_BASEDIRS", "CMAKE_C_COMPILER_LAUNCHER", "CMAKE_CXX_COMPILER_LAUNCHER", "NORTH_DEV_SHELL_TRACE", "NIX_PATH",
];
const MARKS: [&str; 4] = [REMOTE_MARK, SHIP_SECONDS_ENV, SLOT_TARGET_ENV, SLOT_HEAD_ENV];

/// What a sandboxed command sees of this host.
pub struct Layout {
    pub home: PathBuf,
    pub lane: PathBuf,
    pub directory: PathBuf,
    pub runtime: PathBuf,
    pub git: Option<(PathBuf, PathBuf)>,
    pub reads: Vec<PathBuf>,
}

impl Layout {
    /// The layout for `lane` with the command started in `directory`, reading this host's compiler cache, cargo registry and dev-shell roots, and running the north binary at `north`.
    pub fn of(lane: &Path, directory: &Path, home: &Path, north: &Path) -> Self {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let cargo = set("CARGO_HOME").unwrap_or_else(|| home.join(".cargo"));
        let mut reads = vec![cargo.join("registry"), cargo.join("git"), home.join(".local/state/north/dev-shells")];
        reads.extend(set("SCCACHE_DIR"));
        let runtime = north.parent().map_or_else(|| north.to_path_buf(), Path::to_path_buf);
        Self { home: home.to_path_buf(), lane: lane.to_path_buf(), directory: directory.to_path_buf(), runtime, git: git_directories(lane), reads }
    }

    /// The bubblewrap invocation that runs `command` in this layout.
    pub fn command(&self, command: &[String]) -> Command {
        let mut sandbox = Command::new("bwrap");
        sandbox.args(["--unshare-all", "--die-with-parent", "--new-session", "--clearenv", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"]);
        for path in SYSTEM.iter().map(Path::new).filter(|path| path.exists()) {
            sandbox.arg("--ro-bind").arg(path).arg(path);
        }
        sandbox.arg("--tmpfs").arg(&self.home);
        for path in self.reads.iter().chain([&self.runtime]).chain(self.git.as_ref().map(|(common, _)| common)).filter(|path| path.exists()) {
            sandbox.arg("--ro-bind").arg(path).arg(path);
        }
        if let Some((_, own)) = &self.git {
            sandbox.arg("--bind").arg(own).arg(own);
        }
        sandbox.arg("--bind").arg(&self.lane).arg(&self.lane);
        sandbox.arg("--symlink").arg(&self.runtime).arg(self.home.join(".local/state/north/current"));
        for name in PASSED.iter().chain(&MARKS) {
            if let Some(value) = std::env::var_os(name) {
                sandbox.arg("--setenv").arg(name).arg(value);
            }
        }
        let north = self.runtime.join("north");
        let fixed: [(&str, OsString); 6] = [
            ("HOME", self.home.clone().into()),
            ("TMPDIR", "/tmp".into()),
            ("AGENT_CAPACITY_NORTH", north.into()),
            ("CARGO_NET_OFFLINE", "true".into()),
            ("SCCACHE_LOCAL_RW_MODE", "READ_ONLY".into()),
            ("SCCACHE_SERVER_UDS", "/tmp/sccache.sock".into()),
        ];
        for (name, value) in fixed {
            sandbox.arg("--setenv").arg(name).arg(value);
        }
        sandbox.arg("--chdir").arg(&self.directory).arg("--").args(command);
        sandbox
    }
}

/// The lane's shared git directory, mounted read-only, and its own worktree directory inside it, mounted writable so git can refresh the lane's index.
fn git_directories(lane: &Path) -> Option<(PathBuf, PathBuf)> {
    let ask = |flag: &str| {
        let output = Command::new("git").arg("-C").arg(lane).args(["rev-parse", "--path-format=absolute", flag]).output().ok()?;
        output.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
    };
    Some((ask("--git-common-dir")?, ask("--git-dir")?))
}

/// Whether `command` is a sandbox run, which enters its lane's dev shell inside the sandbox itself so the flake's shell hook runs sandboxed too.
pub fn sandboxes(command: &[String]) -> bool {
    matches!(command, [_, capacity, sandbox, ..] if capacity == "capacity" && sandbox == "sandbox")
}

pub fn run(arguments: &[String]) -> u8 {
    let [flag, lane, separator, command @ ..] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    if flag != "--lane" || separator != "--" || command.is_empty() {
        eprintln!("{USAGE}");
        return 2;
    }
    let (Ok(lane), Ok(directory)) = (std::fs::canonicalize(lane), std::env::current_dir()) else {
        eprintln!("north capacity sandbox: {lane} is not a directory");
        return 2;
    };
    if !directory.starts_with(&lane) {
        eprintln!("north capacity sandbox: {} is outside the lane {}", directory.display(), lane.display());
        return 2;
    }
    let prepared = dev_shell::command(&["run".to_owned(), "--dir".to_owned(), directory.to_string_lossy().into_owned(), "--prepare".to_owned()]);
    if prepared != 0 {
        return prepared;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let north = std::env::current_exe().and_then(std::fs::canonicalize).unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let layout = Layout::of(&lane, &directory, &home, &north);
    let inner = dev_shell::enter(&layout.runtime.join("north").to_string_lossy(), command);
    let error = layout.command(&inner).exec();
    eprintln!("north capacity sandbox: bwrap: {error}");
    126
}
