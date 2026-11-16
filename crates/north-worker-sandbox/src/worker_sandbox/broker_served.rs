//! What one broker serves beyond the host's own projects, as the words after `north --worker-sandbox --broker` say it: the projects under one more directory, and the one cgroup it answers when a command started it for a scratch project of its own.

use std::ffi::OsString;
use std::path::PathBuf;

use super::CODE;
use super::peer::Peer;

pub(super) const PEERS: &str = "--peers";

/// What a broker serves beyond the host's own projects: those under one more directory, and, when a command started it for its scratch project, the cgroup (as `/proc/PID/cgroup` reads, trimmed) whose processes alone it answers.
#[derive(Clone, Default)]
pub(super) struct Served {
    pub code: Option<PathBuf>,
    pub peers: Option<String>,
}

impl Served {
    /// The words after `--broker` that start a broker serving this.
    pub(super) fn words(&self) -> Vec<OsString> {
        let code = self.code.iter().flat_map(|code| [OsString::from(CODE), code.clone().into_os_string()]);
        code.chain(self.peers.iter().flat_map(|peers| [OsString::from(PEERS), OsString::from(peers)])).collect()
    }

    /// Why the caller `peer` is not answered: the broker was started for one command, and the caller runs in another cgroup.
    pub(super) fn refusal(&self, peer: &Peer) -> Option<String> {
        let peers = self.peers.as_deref()?;
        (peer.cgroup().map(str::trim) != Some(peers)).then(|| "this broker was started for one command's scratch project and answers only the processes of that command's cgroup; ask the host's broker from your lane".to_owned())
    }
}
