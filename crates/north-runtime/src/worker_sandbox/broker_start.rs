//! The words of `north --worker-sandbox --broker`: the host's broker, a broker that also serves the projects under one more directory, and the start of such a one in a service of its own. The host's broker serves the projects the host keeps and no other, and which projects it serves is not a caller's to say, so a command that makes a scratch project (a canary probe: runtime/canary/probes.rs) starts a broker of its own north for it, on a socket of its own, and asks that one. The scratch project and that socket lie where any session of the host writes, so a broker started this way answers only the processes in the cgroup of the command that started it.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::broker::{self, SOCKET_ENV};
use super::broker_served::{PEERS, Served};
use super::lane_grant::Host;
use super::peer::Peer;
use super::{BROKER, CODE, FLAG, USAGE};

const DETACHED: &str = "--detached";

/// The words after `north` that start a broker for the projects under `code` on the socket the caller's environment names (`detached` below).
pub fn words(code: &str) -> [&str; 5] {
    [FLAG, BROKER, CODE, code, DETACHED]
}

/// Runs `north --worker-sandbox --broker SERVED`.
pub(super) fn run(served: &[String]) -> ExitCode {
    match served.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => serve(Served::default()),
        [CODE, directory] => serve(Served { code: Some(PathBuf::from(directory)), peers: None }),
        [CODE, directory, PEERS, peers] => serve(Served { code: Some(PathBuf::from(directory)), peers: Some(peers.trim().to_owned()) }),
        [CODE, directory, DETACHED] => detached(Path::new(directory)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Serves `served` until the broker's socket is replaced or removed (`broker::serve`).
fn serve(served: Served) -> ExitCode {
    broker::serve(served.words(), std::sync::Arc::new(move |north: &Path, stream: &UnixStream, peer: &Peer| answer(north, stream, peer, &served)))
}

/// Answers the one request on `stream` as a broker that serves `served`, from the caller `peer`: a caller the broker does not answer is told so and started nothing, and its refusal is returned for the broker's log.
pub(super) fn answer(north: &Path, stream: &UnixStream, peer: &Peer, served: &Served) -> Result<(), String> {
    if let Some(why) = served.refusal(peer) {
        let Some((_, streams)) = broker::receive(stream)? else { return Ok(()) };
        let why = broker::refuse(streams, why);
        (&*stream).write_all(&[broker::REFUSED]).map_err(|error| error.to_string())?;
        return Err(why);
    }
    let mut host = Host::own();
    host.code.extend(served.code.clone());
    broker::answer(north, stream, peer, &host)
}

/// Starts this north as a broker that also serves the projects under `code` and answers this process's cgroup alone, in a transient user service as `broker::ensure` starts the host's, on the socket [`SOCKET_ENV`] names; whether one listens there. The host's own socket is never given such a broker. Removing the socket ends it.
fn detached(code: &Path) -> ExitCode {
    let Some(path) = std::env::var_os(SOCKET_ENV).filter(|value| !value.is_empty()).map(PathBuf::from) else {
        eprintln!("north worker-sandbox: a broker for the projects under {} is started on a socket of its own, which {SOCKET_ENV} names", code.display());
        return ExitCode::from(2);
    };
    let Some(peers) = std::fs::read_to_string("/proc/self/cgroup").ok().map(|cgroup| cgroup.trim().to_owned()).filter(|cgroup| !cgroup.is_empty()) else {
        eprintln!("north worker-sandbox: this command cannot read its own cgroup, so a broker for the projects under {} could not tell its requests from another command's", code.display());
        return ExitCode::from(1);
    };
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let served = Served { code: Some(code.to_owned()), peers: Some(peers) };
    broker::ensure_listening(&path, |unit| broker::start(&north, unit, &served.words()));
    if UnixStream::connect(&path).is_ok() {
        return ExitCode::SUCCESS;
    }
    eprintln!("north worker-sandbox: no broker for the projects under {} listens on {}: it starts as a transient user service, which a command that cannot reach the user manager cannot ask for", code.display(), path.display());
    ExitCode::from(1)
}
