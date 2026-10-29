use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::detect::{Action, Cap, message};
use crate::platform::process::status_within;

const COMMAND_WAIT: Duration = Duration::from_secs(15);

pub(super) fn perform(actions: &[Action], incidents: &Path, north: &Path) {
    for action in actions {
        match action {
            Action::Incident(incident) => record(incidents, incident),
            Action::Cap(cap) => limit(cap, &format!("CPUQuota={}%", cap.cpu_quota_percent), &format!("CPUWeight={}", cap.cpu_weight)),
            Action::Release(cap) => limit(cap, "CPUQuota=", "CPUWeight="),
            Action::Send { to, text } => deliver(north, to, text),
        }
    }
}

fn record(path: &Path, incident: &super::detect::Incident) {
    let body = message(incident).lines().skip(1).map(|line| format!("  {line}\n")).collect::<String>();
    let entry = format!("- {} host-overload: load {} ({}/core), cpu pressure {}%\n{body}", incident.at, incident.load1, incident.load_per_core, incident.psi60);
    if let Err(error) = OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| file.write_all(entry.as_bytes())) {
        eprintln!("north capacity watchdog: {}: {error}", path.display());
    }
}

fn limit(cap: &Cap, quota: &str, weight: &str) {
    let mut systemctl = Command::new("systemctl");
    systemctl.args(["--user", "set-property", "--runtime", &cap.unit, quota, weight]);
    finish(&mut systemctl, &format!("set-property {} {quota} {weight}", cap.unit));
}

fn deliver(north: &Path, to: &str, text: &str) {
    let mut send = Command::new(north);
    send.args(["fleet", "send", "--now", to, text]);
    finish(&mut send, &format!("fleet send {to}"));
}

fn finish(command: &mut Command, what: &str) {
    match status_within(command, COMMAND_WAIT) {
        Ok(Some(status)) if status.success() => {}
        Ok(Some(status)) => eprintln!("north capacity watchdog: {what} exited {status}"),
        Ok(None) => eprintln!("north capacity watchdog: {what} gave no answer within {}s", COMMAND_WAIT.as_secs()),
        Err(error) => eprintln!("north capacity watchdog: {what}: {error}"),
    }
}
