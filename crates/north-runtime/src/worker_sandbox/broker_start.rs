//! The words of `north --worker-sandbox --broker`: the host's broker, a broker that also serves the projects under one more directory, and the start of such a one in a service of its own. The host's broker serves the projects the host keeps and no other, and which projects it serves is not a caller's to say, so a command that makes a scratch project (a canary probe: runtime/canary/probes.rs) starts a broker of its own north for it, on a socket of its own, and asks that one.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::broker::{self, SOCKET_ENV};
use super::{BROKER, CODE, FLAG, USAGE};

const DETACHED: &str = "--detached";

/// The words after `north` that start a broker for the projects under `code` on the socket the caller's environment names (`detached` below).
pub fn words(code: &str) -> [&str; 5] {
    [FLAG, BROKER, CODE, code, DETACHED]
}

/// Runs `north --worker-sandbox --broker SERVED`.
pub(super) fn run(served: &[String]) -> ExitCode {
    match served.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => broker::serve(None),
        [CODE, directory] => broker::serve(Some(PathBuf::from(directory))),
        [CODE, directory, DETACHED] => detached(Path::new(directory)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Starts this north as a broker that also serves the projects under `code`, in a transient user service as `broker::ensure` starts the host's, on the socket [`SOCKET_ENV`] names; whether one listens there. The host's own socket is never given such a broker. Removing the socket ends it.
fn detached(code: &Path) -> ExitCode {
    let Some(path) = std::env::var_os(SOCKET_ENV).filter(|value| !value.is_empty()).map(PathBuf::from) else {
        eprintln!("north worker-sandbox: a broker for the projects under {} is started on a socket of its own, which {SOCKET_ENV} names", code.display());
        return ExitCode::from(2);
    };
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    broker::ensure_listening(&path, |unit| broker::start(&north, unit, Some(code)));
    if UnixStream::connect(&path).is_ok() {
        return ExitCode::SUCCESS;
    }
    eprintln!("north worker-sandbox: no broker for the projects under {} listens on {}: it starts as a transient user service, which a command that cannot reach the user manager cannot ask for", code.display(), path.display());
    ExitCode::from(1)
}
