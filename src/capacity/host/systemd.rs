use std::process::{Command, Output, Stdio};

use super::machine::{aggregate_slice, env, native_slice, uid};
use super::policy::aggregate_cpus;

const NATIVE_CPU_WEIGHT: u32 = 200;

/// The manager's private socket reports peer pid 0 inside a PID namespace and fails with ENODATA; without XDG_RUNTIME_DIR the client uses the user bus.
pub(crate) fn client_environment(command: &mut Command) -> &mut Command {
    if env("DBUS_SESSION_BUS_ADDRESS").is_some() {
        command.env_remove("XDG_RUNTIME_DIR").env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{}/bus", uid()));
    }
    command
}

pub(crate) fn systemctl(inherit_stderr: bool, arguments: &[&str]) -> Option<Output> {
    let mut command = Command::new("systemctl");
    command.arg("--user").args(arguments).stdin(Stdio::null()).stdout(Stdio::piped());
    command.stderr(if inherit_stderr { Stdio::inherit() } else { Stdio::null() });
    client_environment(&mut command).output().ok()
}

fn succeeded(output: Option<Output>) -> bool {
    output.is_some_and(|output| output.status.success())
}

fn set_slice(slice: &str, properties: &[&str]) -> bool {
    let mut arguments = vec!["set-property", "--runtime", slice];
    arguments.extend_from_slice(properties);
    succeeded(systemctl(true, &arguments))
}

/// Caps the aggregate batch slice for the profile and weights the native slice above it.
pub(crate) fn apply_profile(profile: &str, cores: f64) -> bool {
    let quota = format!("CPUQuota={}%", super::json::show(aggregate_cpus(profile, cores) * 100.0));
    let weight = format!("CPUWeight={NATIVE_CPU_WEIGHT}");
    let io = format!("IOWeight={NATIVE_CPU_WEIGHT}");
    set_slice(&aggregate_slice(), &[&quota]) && set_slice(&native_slice(), &[&weight, &io])
}

pub(crate) fn scope_unit(id: &str) -> String {
    format!("agent-capacity-{}.scope", id.replace('-', ""))
}

pub(crate) fn scope_live(id: &str) -> bool {
    let state = systemctl(false, &["show", &scope_unit(id), "--property=ActiveState", "--value"])
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    // Unknown manager state must not free a possibly live resource allowance.
    !matches!(state.as_str(), "inactive" | "failed")
}

pub(crate) fn kill_scope(id: &str) -> bool {
    succeeded(systemctl(false, &["kill", "--signal=SIGKILL", &scope_unit(id)]))
}

pub(crate) fn stop_scope(unit: &str) {
    let _ = systemctl(false, &["stop", unit]);
}
