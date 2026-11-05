use std::fs;
use std::path::{Path, PathBuf};

pub fn fail(message: &str, status: i32) -> ! {
    eprintln!("machine-capacity: {message}");
    std::process::exit(status)
}

/// Reads a file, treating an absent or vanished one as None.
pub fn read_text(path: impl AsRef<Path>) -> Option<String> {
    match fs::read(path.as_ref()) {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(error) if matches!(error.raw_os_error(), Some(2 | 3 | 19 | 13 | 20)) => None,
        Err(error) => fail(&format!("{}: {error}", path.as_ref().display()), 1),
    }
}

pub fn host_name() -> String {
    read_text("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned()
}

pub fn runtime_root() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    let base = std::env::var("XDG_RUNTIME_DIR").ok().unwrap_or_else(|| format!("/run/user/{uid}"));
    if !base.starts_with('/') {
        fail("XDG_RUNTIME_DIR must be absolute", 2);
    }
    state_root(&base, uid)
}

/// A private desktop's own runtime directory (`/run/user/UID/private-desktop.XXXX/runtime`) holds no capacity state, so a run inside it reads and writes the host's `/run/user/UID` state.
pub fn state_root(runtime: &str, uid: u32) -> PathBuf {
    let user = format!("/run/user/{uid}");
    let inside_desktop = runtime.strip_prefix(&user).and_then(|rest| rest.strip_prefix("/private-desktop.")).is_some();
    Path::new(if inside_desktop { user.as_str() } else { runtime }).join("agent-capacity-v1")
}
