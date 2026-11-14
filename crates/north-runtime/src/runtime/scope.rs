use std::path::PathBuf;
use std::process::{Command, Stdio};

const SYSTEMD_RUN: &str = "systemd-run";
const SCOPE: [&str; 5] = ["--user", "--scope", "--slice=agent.slice", "--quiet", "--collect"];

/// The lander's unit runs `north land follow` and systemd kills its whole control group when the lander restarts on a new runtime, so the promote runs in a scope of its own, under agent.slice so its runtime build counts against the agent CPU quota: in the default app.slice it compiled outside the 12-CPU desk bound (north#892).
pub(super) fn own_scope(north: PathBuf) -> Command {
    let scopes = Command::new(SYSTEMD_RUN).args(SCOPE).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success());
    if !scopes {
        return Command::new(north);
    }
    let mut command = Command::new(SYSTEMD_RUN);
    command.args(SCOPE).arg("--").arg(north);
    command
}
