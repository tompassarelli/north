use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_foundation::error::{NorthError, NorthResult};
use north_fleet_supervisor::fleet::link::quote;
use north_fleet_supervisor::fleet::model::Machine;
use north_fleet_supervisor::fleet::runtime::binary;
use north_base::platform::process::try_run_command_within;

use super::archive;
use super::lane::Planned;

const WAIT: Duration = Duration::from_secs(120);

/// What a session placed on `machine` needs there before its host starts: the scratch directory the launcher filled on this machine, and the lane planned for it, because the host and its worker run on `machine` while the launcher runs here.
pub(super) fn stage(machine: &Machine, home: &Path, scratch: &Path, lane: Option<&Planned>) -> NorthResult<()> {
    let (parent, directory) = scratch.parent().zip(scratch.file_name()).ok_or_else(|| NorthError::State(format!("{} has no parent directory to stage from", scratch.display())))?;
    let archive = archive::pack(parent, directory)?;
    let script = script(machine, home, parent, lane);
    let mut ssh = Command::new(binary("ssh"));
    ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", machine.session_login(), "--", &script]);
    match try_run_command_within(ssh, Some(&archive), WAIT) {
        Ok(Some(output)) if output.status.success() => Ok(()),
        Ok(Some(output)) => Err(NorthError::Refused(format!("staging the session on {}: {}: {}", machine.name, output.status, String::from_utf8_lossy(&output.stderr).trim()))),
        Ok(None) => Err(NorthError::Refused(format!("staging the session on {}: no answer within {}s", machine.name, WAIT.as_secs()))),
        Err(error) => Err(NorthError::Refused(format!("staging the session on {}: {error}", machine.name))),
    }
}

/// The shell `machine`'s session login runs to unpack the scratch archive under `parent` and make the planned lane, with the lane's checkout and path moved under that login's own home, so an agent login never enters the operator's.
pub(super) fn script(machine: &Machine, home: &Path, parent: &Path, lane: Option<&Planned>) -> String {
    let mut script = format!("mkdir -p {parent} && tar -C {parent} -xf -", parent = quote(&parent.to_string_lossy()));
    if let Some(lane) = lane {
        let (checkout, path) = (machine.session_path(&lane.checkout, home), machine.session_path(&lane.path, home));
        script.push_str(&format!(" && cd {} && {{ [ -d {} ] || north lane new --issue {} {}; }}", quote(&checkout.to_string_lossy()), quote(&path.to_string_lossy()), quote(&lane.issue), quote(&path.to_string_lossy())));
    }
    script
}
