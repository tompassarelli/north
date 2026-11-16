//! The dispatch host's record, pulled by the records mirror run as one fixed file over the login and key the mirror already pushes to that host with, then parsed and written again with only its known fields, so nothing else on that host rides the mirror to the other hosts.
use super::bounded;
use super::remote::OWN_RSH;
use north_base::platform::host;
use north_dispatch::dispatch::{HOST_RECORD, HOST_RECORD_BYTES, dispatch_host_elsewhere, republish_host_record};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const PULLED: &str = "north/dispatch/host.json.pulled";
const DROPPED: &str = "north/dispatch/host.json.dropped";
const PULL: Duration = Duration::from_secs(20);

/// Pulls the dispatch host's record into this host's state when that host is one of the mirror's `destinations`; the line saying why it was not, once per reason.
pub(super) fn carry(state: &Path, destinations: &[String]) -> Option<String> {
    let cfg = north_pacing::pacing::placement::config().ok()?;
    let (name, ssh) = dispatch_host_elsewhere(&cfg, &host::name())?;
    if !destinations.contains(&ssh) {
        return once(state, format!("records mirror: {name}'s dispatch record not pulled: {ssh} is not a mirror destination"));
    }
    let pulled = state.join(PULLED);
    let mut rsync = Command::new("rsync");
    rsync.args(["-t", "--timeout=10", &format!("--max-size={HOST_RECORD_BYTES}"), "-e", OWN_RSH]).arg(format!("{ssh}:.local/state/{HOST_RECORD}")).arg(&pulled).stdin(Stdio::null()).stdout(Stdio::null());
    match bounded::status_keeping_stderr(&mut rsync, PULL, 4) {
        Ok((status, _)) if status.success() => {}
        Ok((status, said)) => return once(state, format!("records mirror: {name}'s dispatch record not pulled: rsync {status}: {}", said.join(" | "))),
        Err(error) => return once(state, format!("records mirror: {name}'s dispatch record not pulled: {error}")),
    }
    let text = fs::read(&pulled);
    let _ = fs::remove_file(&pulled);
    let republished = text.map_err(|_| format!("no file of at most {HOST_RECORD_BYTES} bytes came")).and_then(|text| republish_host_record(&text));
    let written = republished.and_then(|clean| {
        let path = state.join(HOST_RECORD);
        let staged = path.with_extension("json.tmp");
        fs::write(&staged, clean).and_then(|()| fs::rename(&staged, &path)).map_err(|error| format!("{}: {error}", path.display()))
    });
    match written {
        Ok(()) => {
            let _ = fs::remove_file(state.join(DROPPED));
            None
        }
        Err(why) => once(state, format!("records mirror: {name}'s dispatch record dropped: {why}")),
    }
}

fn once(state: &Path, line: String) -> Option<String> {
    let marker = state.join(DROPPED);
    if fs::read_to_string(&marker).is_ok_and(|said| said == line) {
        return None;
    }
    let _ = marker.parent().map(fs::create_dir_all);
    let _ = fs::write(&marker, &line);
    Some(line)
}
