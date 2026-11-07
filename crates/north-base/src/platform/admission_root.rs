//! Where this host's capacity admission keeps its leases and queue, read by the capacity helper that writes them and by the fleet that asks whether a session's build still waits there.
use std::path::{Path, PathBuf};

/// The admission state under `runtime`, the user's runtime directory; a private desktop's own runtime directory (`/run/user/UID/private-desktop.XXXX/runtime`) holds none, so a run inside it reads and writes the host's `/run/user/UID` state.
pub fn state_root(runtime: &str, uid: u32) -> PathBuf {
    let user = format!("/run/user/{uid}");
    let inside_desktop = runtime.strip_prefix(&user).and_then(|rest| rest.strip_prefix("/private-desktop.")).is_some();
    Path::new(if inside_desktop { user.as_str() } else { runtime }).join("agent-capacity-v1")
}

/// This process's admission state, from `XDG_RUNTIME_DIR` or the user's `/run/user/UID`.
pub fn here() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    state_root(&std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{uid}")), uid)
}
