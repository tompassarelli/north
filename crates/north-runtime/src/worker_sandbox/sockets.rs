use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use rustix::mount::{MountPropagationFlags, mount_bind, mount_change};
use rustix::thread::{LinkNameSpaceType, UnshareFlags, move_into_link_name_space, unshare};

use north_base::foundation::offload::MASTER_SOCKET;

use super::Context;

const HIDDEN_NORTH: [&str; 5] = [north_base::platform::ssh::HOST_MASTER, "north-taker-", "north-links", "north-spares", "north-session-hosts"];
const KEPT: [&str; 6] = ["wayland-", "pipewire-", "pulse", "private-desktop", "cc-socks", "north"];
const DEPTH: usize = 4;
const SSH_CONFIG: &str = "/etc/ssh/ssh_config";
pub const AGENT_ENV: &str = "SSH_AUTH_SOCK";
/// What `make` prints when its namespaces are laid.
pub const READY: &[u8] = b"ready\n";

pub fn runtime(context: &Context) -> PathBuf {
    context.runtime.clone().unwrap_or_else(|| context.user_runtime.clone())
}

/// Where the terminal multiplexer keeps its server sockets: `TMUX_TMPDIR`, or `/tmp`, beneath `tmux-UID`.
pub fn multiplexer(uid: u32, tmpdir: Option<OsString>) -> PathBuf {
    tmpdir.filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from("/tmp"), PathBuf::from).join(format!("tmux-{uid}"))
}

/// The sockets a worker command may not connect to: every socket in the runtime directory other than those of the display and audio servers, north's own and Claude Code's proxy, the sockets of the terminal multiplexer, and the `ssh` ones through which ssh reaches another host without reading a key, the offload, host, taker and fleet-link masters and the spare shells' holders among them whatever north's own names keep. The kernel's Landlock before ABI 9 cannot refuse a connect(2), so `hide` puts a null device over each.
pub fn hidden(runtime: &Path, multiplexer: &Path, ssh: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(runtime).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(MASTER_SOCKET) || HIDDEN_NORTH.iter().any(|prefix| name.starts_with(prefix)) || !KEPT.iter().any(|prefix| name.starts_with(prefix)) {
            sockets(&entry.path(), DEPTH, &mut found);
        }
    }
    sockets(multiplexer, DEPTH, &mut found);
    found.extend(ssh.iter().filter(|path| !found.contains(path)).cloned().collect::<Vec<_>>());
    found
}

/// The sockets through which ssh reaches another host without reading a key (south#118): the agent socket `agent` names, and every control-master socket a `ControlPath` of ssh's system or user configuration names, matched by the fixed part of the path before its first token.
pub fn ssh_reach(home: &Path, uid: u32, agent: Option<&Path>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(agent) = agent {
        sockets(agent, 0, &mut found);
    }
    for text in [ssh_config(), Some(home.join(".ssh/config"))].into_iter().flatten().filter_map(|path| std::fs::read_to_string(path).ok()) {
        for pattern in control_paths(&text, home, uid) {
            masters(&pattern, &mut found);
        }
    }
    found.sort();
    found.dedup();
    found
}

fn control_paths(text: &str, home: &Path, uid: u32) -> Vec<String> {
    let value = |line: &str| {
        let (key, rest) = line.trim().split_once(|character: char| character.is_whitespace() || character == '=')?;
        let rest = rest.trim_start_matches(|character: char| character.is_whitespace() || character == '=').trim().trim_matches('"');
        (key.eq_ignore_ascii_case("controlpath") && !rest.is_empty() && !rest.eq_ignore_ascii_case("none")).then(|| rest.to_owned())
    };
    text.lines().filter_map(value).map(|path| expand(&path, home, uid)).collect()
}

fn expand(path: &str, home: &Path, uid: u32) -> String {
    let home = home.to_string_lossy();
    let path = path.strip_prefix("~/").map_or_else(|| path.to_owned(), |rest| format!("{home}/{rest}"));
    path.replace("%i", &uid.to_string()).replace("%d", &home)
}

fn masters(pattern: &str, found: &mut Vec<PathBuf>) {
    let fixed = pattern.split('%').next().unwrap_or_default();
    if fixed.len() == pattern.len() {
        sockets(Path::new(pattern), 0, found);
        return;
    }
    let Some((directory, prefix)) = fixed.rsplit_once('/') else { return };
    for entry in std::fs::read_dir(if directory.is_empty() { "/" } else { directory }).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            sockets(&entry.path(), DEPTH, found);
        }
    }
}

fn sockets(path: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(kind) = std::fs::symlink_metadata(path).map(|metadata| metadata.file_type()) else { return };
    if kind.is_socket() {
        found.push(path.to_path_buf());
    } else if kind.is_dir() && depth > 0 {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            sockets(&entry.path(), depth - 1, found);
        }
    }
}

/// Puts a null device over each of `paths` in a user and mount namespace of this process, which maps the caller's own user and group only; every command it then starts sees a refused connection there. The overlay cannot be lifted from inside the Landlock domain applied after it, which refuses mount(2), and the domain's ptrace rule keeps the original sockets out of reach through the `/proc` entries of processes outside it. Needs a single-threaded process.
pub fn hide(runtime: &Path, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let (uid, gid) = (rustix::process::getuid().as_raw(), rustix::process::getgid().as_raw());
    unshare(UnshareFlags::NEWUSER | UnshareFlags::NEWNS).map_err(|error| format!("unshare: {error}"))?;
    let write = |file: &str, text: String| std::fs::write(format!("/proc/self/{file}"), text).map_err(|error| format!("{file}: {error}"));
    write("setgroups", "deny".to_owned())?;
    write("uid_map", format!("{uid} {uid} 1"))?;
    write("gid_map", format!("{gid} {gid} 1"))?;
    mount_change("/", MountPropagationFlags::SLAVE | MountPropagationFlags::REC).map_err(|error| format!("mount /: {error}"))?;
    let mut failed: Vec<String> = paths.iter().filter_map(|path| mount_bind("/dev/null", path).err().map(|error| format!("{}: {error}", path.display()))).collect();
    failed.extend(own_includes_only(runtime).err());
    if failed.is_empty() { Ok(()) } else { Err(failed.join("; ")) }
}

/// The file ssh reads its system configuration from, with every link resolved: the overlay goes over this path.
pub fn ssh_config() -> Option<PathBuf> {
    std::fs::canonicalize(SSH_CONFIG).ok()
}

// ssh's config format refuses an Included file that this namespace shows as owned by nobody, which is every file of the store, so ssh's system configuration is laid over with a copy that has no Include.
fn own_includes_only(runtime: &Path) -> Result<(), String> {
    let Some(real) = ssh_config() else { return Ok(()) };
    let Ok(text) = std::fs::read_to_string(&real) else { return Ok(()) };
    let kept: String = text.lines().filter(|line| !line.trim_start().to_ascii_lowercase().starts_with("include")).map(|line| format!("{line}\n")).collect();
    if kept.len() == text.len() {
        return Ok(());
    }
    let directory = runtime.join("north-sandbox");
    let copy = directory.join(format!("ssh_config-{:016x}", kept.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))));
    if !copy.exists() {
        let partial = directory.join(format!("ssh_config.{}", std::process::id()));
        std::fs::create_dir_all(&directory).and_then(|()| std::fs::write(&partial, &kept)).and_then(|()| std::fs::rename(&partial, &copy)).map_err(|error| format!("{}: {error}", copy.display()))?;
    }
    mount_bind(&copy, &real).map_err(|error| format!("{}: {error}", real.display()))
}

/// Moves this single-threaded process into the user and mount namespaces whose descriptors the broker passed, the namespaces `hide` made in a process outside every domain, and back into its working directory. A process inside a Landlock domain cannot make such an overlay itself (the domain refuses mount(2)), but nothing refuses it joining one. The overlay cannot be lifted from there for the same reason.
pub fn join(user: BorrowedFd<'_>, mount: BorrowedFd<'_>) -> Result<(), String> {
    let directory = std::env::current_dir().ok();
    move_into_link_name_space(user, Some(LinkNameSpaceType::User)).map_err(|error| format!("setns user: {error}"))?;
    move_into_link_name_space(mount, Some(LinkNameSpaceType::Mount)).map_err(|error| format!("setns mount: {error}"))?;
    directory.map_or(Ok(()), std::env::set_current_dir).map_err(|error| format!("chdir: {error}"))
}

/// The broker's namespace maker: hides `paths` as `hide` does, says it is ready, and holds the namespaces open until its standard input closes.
pub fn make(runtime: &Path, paths: &[PathBuf]) -> std::process::ExitCode {
    if let Err(error) = hide(runtime, paths) {
        eprintln!("north worker-sandbox: {error}");
        return std::process::ExitCode::from(1);
    }
    let _ = std::io::stdout().write_all(READY).and_then(|()| std::io::stdout().flush());
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    std::process::ExitCode::SUCCESS
}
