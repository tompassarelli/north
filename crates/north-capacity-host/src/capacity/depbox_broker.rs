use std::ffi::OsString;
use std::path::{Path, PathBuf};

use north_base::platform::broker_client;

fn directories() -> Vec<PathBuf> {
    let named = std::env::var_os(broker_client::SOCKET_ENV).map(PathBuf::from).and_then(|socket| socket.parent().map(Path::to_path_buf));
    let system = Path::new(broker_client::SYSTEM_SOCKET).parent().map(Path::to_path_buf);
    let mut found: Vec<PathBuf> = named.into_iter().chain(system).filter(|directory| directory.is_dir() && directory.parent().is_some()).collect();
    found.sort();
    found.dedup();
    found
}

pub fn hides(path: &Path) -> bool {
    directories().iter().any(|hidden| path.starts_with(hidden))
}

pub fn overlay(top: &Path, sandbox: &Path) -> Vec<OsString> {
    let mut arguments = Vec::new();
    for directory in directories() {
        let inside = directory.strip_prefix(top).ok().map(|relative| sandbox.join(relative));
        for hidden in std::iter::once(directory).chain(inside) {
            arguments.extend([OsString::from("--tmpfs"), hidden.into()]);
        }
    }
    arguments
}
