//! The words of `north --worker-sandbox --broker`: the host's broker, or one that also serves the projects under one more directory. The host's broker serves the projects the host keeps and no other, and which projects it serves is not a caller's to say.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::broker;
use super::broker_served::Served;
use super::lane_grant::Host;
use super::peer::Peer;
use super::{CODE, Carry, USAGE};

/// Runs `north --worker-sandbox --broker SERVED`.
pub(super) fn run(served: &[String], carry: Carry) -> ExitCode {
    match served.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => serve(Served::default(), carry),
        [CODE, directory] => serve(Served { code: Some(PathBuf::from(directory)) }, carry),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Serves `served` until the broker's socket is replaced or removed (`broker::serve`).
fn serve(served: Served, carry: Carry) -> ExitCode {
    broker::serve(served.words(), std::sync::Arc::new(move |north: &Path, stream: &UnixStream, peer: &Peer| answer(north, stream, peer, &served, carry)))
}

/// Answers the one request on `stream` as a broker that serves `served`, from the caller `peer`.
pub(super) fn answer(north: &Path, stream: &UnixStream, peer: &Peer, served: &Served, carry: Carry) -> Result<(), String> {
    let mut host = Host::own();
    host.code.extend(served.code.clone());
    broker::answer(north, stream, peer, &host, carry)
}

