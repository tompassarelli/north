use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use north_base::foundation::offload;

const READY_WITHIN: Duration = Duration::from_secs(10);
const READY_POLL: Duration = Duration::from_millis(100);

fn control_path() -> String {
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok().filter(|directory| !directory.is_empty()).unwrap_or_else(|| format!("/run/user/{}", rustix::process::getuid().as_raw()));
    format!("{runtime}/{}%C", offload::MASTER_SOCKET)
}

fn unit_name(ssh: &str) -> String {
    format!("north-offload-master-{}", ssh.chars().map(|letter| if letter.is_ascii_alphanumeric() { letter } else { '-' }).collect::<String>())
}

/// The systemd-run arguments that start the offload master for `ssh` as the main process of a user unit of its own, so stopping the unit of whichever job needed it first never kills the connection every other job on that host rides (north#932).
pub(super) fn unit_arguments(ssh: &str, control_path: &str, agent: Option<&str>) -> Vec<String> {
    let mut arguments: Vec<String> = ["--user", "--collect", "--quiet", "--unit"].map(str::to_owned).into();
    arguments.push(unit_name(ssh));
    if let Some(socket) = agent {
        arguments.push(format!("--setenv=SSH_AUTH_SOCK={socket}"));
    }
    arguments.extend(["--", offload::ssh().as_str(), "-M", "-N", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", "-o"].map(str::to_owned));
    arguments.push(format!("ControlPath={control_path}"));
    arguments.push(ssh.to_owned());
    arguments
}

fn alive(ssh: &str, control_path: &str) -> bool {
    Command::new(offload::ssh()).args(["-O", "check", "-o", "BatchMode=yes", "-o"]).arg(format!("ControlPath={control_path}")).arg(ssh).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

/// The ssh options of an offload client: it rides the host's offload master when one runs, starting it in its own unit first. A client never forks a master itself (`ControlMaster=no`), since a master forked by a client lives in the client's unit; without a master it makes its own connection.
pub(super) fn options(ssh: &str) -> Vec<String> {
    let path = control_path();
    if !ssh.is_empty() && !alive(ssh, &path) {
        let agent = std::env::var("SSH_AUTH_SOCK").ok().filter(|socket| !socket.is_empty());
        let _ = Command::new("systemd-run").args(unit_arguments(ssh, &path, agent.as_deref())).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        let started = Instant::now();
        while started.elapsed() < READY_WITHIN && !alive(ssh, &path) {
            std::thread::sleep(READY_POLL);
        }
    }
    ["ControlMaster=no".to_owned(), format!("ControlPath={path}")].into_iter().flat_map(|option| ["-o".to_owned(), option]).collect()
}
