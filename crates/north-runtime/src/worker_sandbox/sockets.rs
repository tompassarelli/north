use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use rustix::mount::{MountPropagationFlags, mount_bind, mount_change};
use rustix::thread::{UnshareFlags, unshare};

use super::Context;

const KEPT: [&str; 6] = ["wayland-", "pipewire-", "pulse", "private-desktop", "cc-socks", "north"];
const DEPTH: usize = 4;

pub fn runtime(context: &Context) -> PathBuf {
    context.runtime.clone().unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", context.uid)))
}

/// The sockets a worker command may not connect to: every socket in the runtime directory other than those of the display and audio servers, north's own and Claude Code's proxy, and the sockets of the terminal multiplexer. The kernel's Landlock before ABI 9 cannot refuse a connect(2), so `hide` puts a null device over each.
pub fn hidden(context: &Context) -> Vec<PathBuf> {
    let runtime = runtime(context);
    let multiplexer = std::env::var_os("TMUX_TMPDIR").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from("/tmp"), PathBuf::from).join(format!("tmux-{}", context.uid));
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&runtime).into_iter().flatten().flatten() {
        if !KEPT.iter().any(|prefix| entry.file_name().to_string_lossy().starts_with(prefix)) {
            sockets(&entry.path(), DEPTH, &mut found);
        }
    }
    sockets(&multiplexer, DEPTH, &mut found);
    found
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

// ssh refuses an Included file that this namespace shows as owned by nobody, which is every file of the store, so ssh's system configuration is laid over with a copy that has no Include.
fn own_includes_only(runtime: &Path) -> Result<(), String> {
    let Ok(real) = std::fs::canonicalize("/etc/ssh/ssh_config") else { return Ok(()) };
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
