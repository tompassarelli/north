use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::link::quote;
use crate::fleet::model::Machine;
use crate::fleet::runtime::binary;
use crate::platform::process::try_run_command_within;

use super::lane::Planned;

const WAIT: Duration = Duration::from_secs(120);

/// What a session placed on `machine` needs there before its host starts: the scratch directory the launcher filled on this machine, and the lane planned for it, because the host and its worker run on `machine` while the launcher runs here.
pub(super) fn stage(machine: &Machine, scratch: &Path, lane: Option<&Planned>) -> NorthResult<()> {
    let (parent, directory) = scratch.parent().zip(scratch.file_name()).ok_or_else(|| NorthError::State(format!("{} has no parent directory to stage from", scratch.display())))?;
    let archive = Command::new("tar").arg("-C").arg(parent).arg("-cf").arg("-").arg(directory).stdin(Stdio::null()).stderr(Stdio::inherit()).output()?;
    if !archive.status.success() {
        return Err(NorthError::State(format!("tar of {} failed: {}", scratch.display(), archive.status)));
    }
    let mut script = format!("mkdir -p {parent} && tar -C {parent} -xf -", parent = quote(&parent.to_string_lossy()));
    if let Some(lane) = lane {
        script.push_str(&format!(" && cd {} && {{ [ -d {} ] || north lane new --issue {} {}; }}", quote(&lane.checkout.to_string_lossy()), quote(&lane.path.to_string_lossy()), quote(&lane.issue), quote(&lane.path.to_string_lossy())));
    }
    let mut ssh = Command::new(binary("ssh"));
    ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", &machine.ssh, "--", &script]);
    match try_run_command_within(ssh, Some(&archive.stdout), WAIT) {
        Ok(Some(output)) if output.status.success() => Ok(()),
        Ok(Some(output)) => Err(NorthError::Refused(format!("staging the session on {}: {}: {}", machine.name, output.status, String::from_utf8_lossy(&output.stderr).trim()))),
        Ok(None) => Err(NorthError::Refused(format!("staging the session on {}: no answer within {}s", machine.name, WAIT.as_secs()))),
        Err(error) => Err(NorthError::Refused(format!("staging the session on {}: {error}", machine.name))),
    }
}
