//! `north depbox fetch|run -- COMMAND...`: runs the code a dependency brings (build scripts, proc macros, test binaries, install scripts) in a bubblewrap namespace with an empty home, runtime directory and secrets directory, where the checkout and the package caches are the only writable paths and the toolchain directories of home are read-only; `run` has no network, `fetch` shares it for installs and fetches. A call already inside a sandbox of the same mode runs its command directly, so a shim or land check nested in depbox enters no second sandbox.
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::carry::{self, Boundary};
use super::effects::Host;
use super::depbox_mode::Mode;
use super::gate::{self, LEASED};

const USAGE: &str = "usage: north depbox fetch|run -- <command> [args...]\n  fetch  network shared, for installs and fetches\n  run    no network, for builds and tests";
pub const MARK: &str = "DEPBOX";
const ADMITTED: &str = "DEPBOX_ADMITTED";
const WORKER_SANDBOXED: &str = "NORTH_WORKER_SANDBOXED";
const FETCHED: &str = ".local/state/depbox/cargo-fetched";
const UNSHARE: &str = "/run/current-system/sw/bin/unshare";
const CACHES: [&str; 4] = [".bun/install/cache", ".cargo/registry", ".cargo/git", ".cache/nix"];
const BUILD_CACHES: [&str; 2] = [".cache/sccache", ".cache/north/head-north"];
const READS: [&str; 14] = [
    ".cargo/bin", ".rustup", ".gitconfig", ".config/git", ".nix-profile", ".local/bin", ".local/share/north", ".local/state/north/runtimes", ".local/state/north/dev-shells", ".local/state/nix/profile", ".local/state/nix/profiles", ".local/lib/firn/cli", "code/nixos-config/pins", "code/north/pins",
];

pub fn run(arguments: &[String], host: &Host) -> u8 {
    if let Some(inside) = std::env::var(MARK).ok().filter(|value| !value.is_empty()) {
        let (mode, top) = inside.split_once(':').unwrap_or((&inside, ""));
        if let [word, marker, command @ ..] = arguments
            && word == mode
            && marker == "--"
            && let Some((program, rest)) = command.split_first()
        {
            return exec(Command::new(program).args(rest), program);
        }
        eprintln!("depbox: already inside the depbox {mode} sandbox of {top}; run the command directly");
        return 2;
    }
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    if std::env::var_os(WORKER_SANDBOXED).is_some_and(|value| !value.is_empty()) {
        return exec(Command::new(&north).args(["--worker-sandbox", "--outside", "--"]).arg(&north).arg("depbox").args(arguments), "north --worker-sandbox --outside");
    }
    if std::env::var_os(ADMITTED).is_none() {
        let seeded = Command::new(&north).args(["lane", "seed"]).env_remove("CARGO_TARGET_DIR").status();
        if let Some(code) = failed(seeded) {
            return code;
        }
        let words: Vec<String> = std::iter::once("depbox".to_owned()).chain(arguments.iter().cloned()).collect();
        if let Some(class) = gate::lease_class(&words, host) {
            let mut leased = Command::new(&north);
            leased.env(ADMITTED, "1").args(["capacity", "host", "run", "--class", class.name(), "--owner", &gate::owner(), "--timeout-seconds", "3600", "--"]).arg(&north).arg("depbox").args(arguments);
            return exec(&mut leased, "north capacity host run");
        }
    }
    if !owns_pid_namespace() {
        return exec(Command::new(UNSHARE).args(["--user", "--map-current-user", "--pid", "--fork", "--kill-child=KILL", "--mount-proc"]).arg(&north).arg("depbox").args(arguments), UNSHARE);
    }
    let (mode, command) = match arguments {
        [mode, marker, command @ ..] if marker == "--" && !command.is_empty() && let Some(mode) = Mode::parse(mode) => (mode, command),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match Sandbox::here(mode) {
        Ok(sandbox) => sandbox.enter(command),
        Err((code, message)) => {
            eprintln!("depbox: {message}");
            code
        }
    }
}

/// What a sandbox binds of this host: the checkout, the same checkout again at its project's stable sandbox path (so every lane of a project builds into one `target` path that cargo's fingerprints accept), the shared git directory and home's caches and toolchains.
struct Sandbox {
    mode: Mode,
    top: PathBuf,
    sandbox: PathBuf,
    home: PathBuf,
    named_home: PathBuf,
    runtime: PathBuf,
    arguments: Vec<OsString>,
    binds: Vec<PathBuf>,
}

impl Sandbox {
    fn here(mode: Mode) -> Result<Self, (u8, String)> {
        let top = git(&["rev-parse", "--show-toplevel"]).and_then(|top| std::fs::canonicalize(top).ok()).ok_or((2, "run inside a git checkout".to_owned()))?;
        let named_home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let home = std::fs::canonicalize(&named_home).map_err(|error| (1, format!("HOME {}: {error}", named_home.display())))?;
        if top == Path::new("/") || top == home {
            return Err((2, format!("refusing to expose {}", top.display())));
        }
        let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).and_then(|common| std::fs::canonicalize(common).ok()).ok_or((2, "the checkout has no git directory".to_owned()))?;
        let sandbox = home.join("code").join(project(&common)).join("sandbox");
        let runtime = PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw()));
        let mut box_ = Self { mode, top: top.clone(), sandbox: sandbox.clone(), home: home.clone(), named_home: named_home.clone(), runtime: runtime.clone(), arguments: Vec::new(), binds: vec![top, sandbox, common] };
        box_.add(&["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "--tmpfs"]);
        box_.arguments.push(runtime.into());
        box_.add(&["--tmpfs"]);
        box_.arguments.push(home.clone().into());
        if Path::new("/run/secrets.d").is_dir() {
            box_.add(&["--tmpfs", "/run/secrets.d"]);
        }
        if named_home != home {
            box_.add(&["--tmpfs"]);
            box_.arguments.push(named_home.into());
        }
        let builds: &[&str] = if mode == Mode::Run { &BUILD_CACHES } else { &[] };
        for directory in CACHES.iter().chain(builds).map(|directory| home.join(directory)) {
            std::fs::create_dir_all(&directory).map_err(|error| (1, format!("{}: {error}", directory.display())))?;
            box_.bind("--bind", &directory, &directory);
            box_.binds.push(directory);
        }
        for path in READS.iter().map(|path| home.join(path)).filter(|path| path.exists()) {
            box_.bind("--ro-bind", &path, &path);
            box_.binds.push(resolved(&path));
            box_.binds.push(path);
        }
        let live = home.join(".local/state/north/current");
        if let Ok(target) = std::fs::read_link(&live) {
            box_.bind("--symlink", &target, &live);
            box_.binds.push(resolved(&live));
            box_.binds.push(live);
        }
        Ok(box_)
    }

    fn add(&mut self, words: &[&str]) {
        self.arguments.extend(words.iter().map(OsString::from));
    }

    fn bind(&mut self, flag: &str, source: &Path, destination: &Path) {
        self.arguments.extend([OsString::from(flag), source.into(), destination.into()]);
    }

    /// Whether a program at `path` is still there inside the sandbox, which hides /tmp, the runtime directory, home and the secrets directory apart from what it binds.
    fn visible(&self, path: &Path) -> bool {
        self.binds.iter().any(|bind| path.starts_with(bind)) || ![Path::new("/tmp"), &self.runtime, &self.home, &self.named_home, Path::new("/run/secrets.d")].iter().any(|hidden| path.starts_with(hidden))
    }

    fn enter(self, command: &[String]) -> u8 {
        let Some(bwrap) = on_path("bwrap") else {
            eprintln!("depbox: bwrap not found on PATH");
            return 1;
        };
        let Some(program) = on_path(&command[0]) else {
            eprintln!("depbox: {}: command not found", command[0]);
            return 127;
        };
        if !(self.visible(&std::path::absolute(&program).unwrap_or_else(|_| program.clone())) && self.visible(&resolved(&program))) {
            let binds: Vec<String> = self.binds.iter().map(|bind| bind.display().to_string()).collect();
            eprintln!("depbox: {} is hidden inside the sandbox; under /tmp, /run/user and the home directory it sees only {}", program.display(), binds.join(" "));
            return 2;
        }
        let fetched = self.home.join(FETCHED);
        let lock = fingerprint(&self.top.join("Cargo.lock"));
        if self.mode == Mode::Run && lock.as_ref().is_some_and(|lock| !fetched.join(lock).exists()) && gate::compiles_rust(command) {
            self.fetch_crates();
        }
        let target = self.top.join("target");
        if let Some(chosen) = std::env::var_os("CARGO_TARGET_DIR").filter(|chosen| !chosen.is_empty())
            && resolved(Path::new(&chosen)) != target
        {
            eprintln!("depbox: ignoring CARGO_TARGET_DIR {}; building in {}", chosen.to_string_lossy(), target.display());
        }
        let records = self.mode == Mode::Fetch && Path::new(&command[0]).file_name() == Some(OsStr::new("cargo"));
        let lock_path = self.top.join("Cargo.lock");
        let mut bubblewrap = self.command(&bwrap, command);
        let Some(lock) = lock.filter(|_| records) else { return exec(&mut bubblewrap, "bwrap") };
        let before = std::fs::read_to_string(&lock_path).ok();
        let code = failed(bubblewrap.status()).unwrap_or(0);
        if let Some(before) = before.filter(|before| std::fs::read_to_string(&lock_path).ok().as_ref() != Some(before))
            && !hold_floor(&lock_path, &before)
        {
            return 1;
        }
        if code == 0 && let Err(error) = std::fs::create_dir_all(&fetched).and_then(|()| std::fs::write(fetched.join(&lock), "")) {
            eprintln!("depbox: {}: {error}", fetched.join(lock).display());
            return 1;
        }
        code
    }

    /// Fetches the crates of a Cargo.lock this host never fetched, in a `fetch` sandbox of its own, before a `run` sandbox without network compiles against it.
    fn fetch_crates(&self) {
        eprintln!("depbox: Cargo.lock in {} was never fetched on this host; fetching its crates first", self.top.display());
        let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
        let fetched = Command::new(north).args(["depbox", "fetch", "--", "cargo", "fetch", "--locked"]).env(ADMITTED, "1").current_dir(&self.top).status();
        if let Some(code) = failed(fetched) {
            eprintln!("depbox: cargo fetch --locked exited {code}; the build runs with the crates this host has");
        }
    }

    /// The bubblewrap call that runs `command` with the checkout bound at both paths, the environment cleared to the carried variables and the working directory at its place under the sandbox path.
    fn command(mut self, bwrap: &Path, command: &[String]) -> Command {
        let (top, sandbox) = (self.top.clone(), self.sandbox.clone());
        self.bind("--bind", &top, &top);
        self.bind("--bind", &top, &sandbox);
        let common = self.binds[2].clone();
        self.bind("--ro-bind", &common, &common);
        self.add(&["--unshare-pid", "--unshare-ipc"]);
        if self.mode == Mode::Run {
            self.add(&["--unshare-net"]);
        }
        self.add(&["--die-with-parent", "--new-session", "--clearenv"]);
        self.set(MARK, OsString::from(format!("{}:{}", self.mode.name(), top.display())));
        if std::env::var_os(ADMITTED).is_some() || gate::in_lease() {
            self.set(LEASED, OsString::from("1"));
        }
        let boundary = if self.mode == Mode::Run { Boundary::Sandbox } else { Boundary::Fetch };
        for name in carry::names(boundary) {
            if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
                self.set(name, value);
            }
        }
        self.set("CARGO_TARGET_DIR", sandbox.join("target").into());
        let here = std::env::current_dir().and_then(std::fs::canonicalize).unwrap_or_else(|_| top.clone());
        let directory = here.strip_prefix(&top).map_or(here.clone(), |relative| sandbox.join(relative));
        self.arguments.extend([OsString::from("--chdir"), directory.into(), OsString::from("--")]);
        let mut bubblewrap = Command::new(bwrap);
        bubblewrap.args(&self.arguments).args(command);
        bubblewrap
    }

    fn set(&mut self, name: &str, value: OsString) {
        self.arguments.extend([OsString::from("--setenv"), OsString::from(name), value]);
    }
}

/// The project a git common directory belongs to: `code/<project>/<checkout>/.git` names `<project>`, a bare common directory its own name.
fn project(common: &Path) -> OsString {
    let container = if common.file_name() == Some(OsStr::new(".git")) { common.parent().and_then(Path::parent) } else { None };
    container.unwrap_or(common).file_name().map(OsStr::to_os_string).unwrap_or_default()
}

/// Whether this process sees itself under its own pid in /proc; a process in a pid namespace whose /proc belongs to another needs a namespace of its own before bubblewrap can mount one.
/// Whether a Cargo.lock a fetch-mode cargo command rewrote keeps every entry it resolved at least 7 days old; `north land scan dep-floor` restores `before` and names the young entries when not.
fn hold_floor(lock: &Path, before: &str) -> bool {
    use std::io::Write as _;
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let child = Command::new(north).args(["land", "scan", "dep-floor"]).arg(lock).stdin(std::process::Stdio::piped()).spawn();
    let Ok(mut child) = child else {
        eprintln!("depbox: cannot start north land scan dep-floor; {} keeps what the command resolved", lock.display());
        return false;
    };
    let _ = child.stdin.take().map(|mut stdin| stdin.write_all(before.as_bytes()));
    child.wait().is_ok_and(|status| status.success())
}

fn owns_pid_namespace() -> bool {
    std::fs::read_to_string("/proc/self/stat").ok().and_then(|stat| stat.split_whitespace().next()?.parse::<u32>().ok()) == Some(std::process::id())
}

fn fingerprint(lock: &Path) -> Option<String> {
    let digest = Sha256::digest(std::fs::read(lock).ok()?);
    Some(digest[..8].iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

fn on_path(program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let runnable = |path: &Path| std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0);
    if program.contains('/') {
        return runnable(Path::new(program)).then(|| PathBuf::from(program));
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(program)).find(|candidate| runnable(candidate))
}

/// `path` with every link resolved, its missing tail kept as written.
fn resolved(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut existing = absolute.as_path();
    let mut tail = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(existing) {
            return tail.iter().rev().fold(real, |path, part| path.join(part));
        }
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else { return absolute };
        tail.push(name.to_owned());
        existing = parent;
    }
}

fn failed(status: std::io::Result<std::process::ExitStatus>) -> Option<u8> {
    use std::os::unix::process::ExitStatusExt;
    match status {
        Ok(status) if status.success() => None,
        Ok(status) => Some(status.code().map_or_else(|| 128 + status.signal().unwrap_or(0) as u8, |code| code as u8)),
        Err(error) => {
            eprintln!("depbox: {error}");
            Some(126)
        }
    }
}

fn exec(command: &mut Command, program: &str) -> u8 {
    let error = command.exec();
    eprintln!("depbox: {program}: {error}");
    if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
}
