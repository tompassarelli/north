use std::process::{Command, Output, Stdio};

use super::machine::{aggregate_slice, batch_slice, env, native_slice, uid};
use crate::capacity::admission::default_mib;
use crate::capacity::cpus::aggregate_cpus;

const NATIVE_CPU_WEIGHT: u32 = 200;
const DESKTOP_SLICE: &str = "app.slice";
const SESSION_SLICE: &str = "session.slice";
const AGENT_SLICE: &str = "agent.slice";

/// The manager's private socket reports peer pid 0 inside a PID namespace and fails with ENODATA; without XDG_RUNTIME_DIR the client uses the user bus.
pub fn client_environment(command: &mut Command) -> &mut Command {
    if env("DBUS_SESSION_BUS_ADDRESS").is_some() {
        command.env_remove("XDG_RUNTIME_DIR").env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{}/bus", uid()));
    }
    command
}

/// A systemd tool by the name a unit's PATH gives it, else the system profile's, since a unit that lists only its own tools (firn's job units) reaches no systemctl or systemd-run and so could never take a lease.
pub fn program(name: &str) -> std::path::PathBuf {
    program_in(name, env("PATH").as_deref(), std::path::Path::new("/run/current-system/sw/bin"))
}

pub(super) fn program_in(name: &str, path: Option<&str>, system: &std::path::Path) -> std::path::PathBuf {
    let on_path = path.is_some_and(|path| path.split(':').any(|directory| !directory.is_empty() && std::path::Path::new(directory).join(name).is_file()));
    let fallback = system.join(name);
    if !on_path && fallback.is_file() { fallback } else { name.into() }
}

pub fn systemctl(inherit_stderr: bool, arguments: &[&str]) -> Option<Output> {
    let mut command = Command::new(program("systemctl"));
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

pub(super) fn desktop_weights(profile: &str) -> [(&'static str, u32); 3] {
    let weight = |name: &str| default_mib(name) as u32;
    let attended = profile == "attended";
    let app = weight(if attended { "slice_weight_app_attended" } else { "slice_weight_app_unattended" });
    let session = if attended { app } else { weight("slice_weight_session_unattended") };
    [(DESKTOP_SLICE, app), (SESSION_SLICE, session), (AGENT_SLICE, weight("slice_weight_agent"))]
}

/// Caps the aggregate slice for the profile, makes the batch slice idle-weighted with no quota, weights the native slice above both and sets the desktop, session and agent slice weights for the profile.
pub fn apply_profile(profile: &str, cores: f64, slice_cpus: Option<f64>) -> bool {
    let quota = format!("CPUQuota={}%", super::json::show(aggregate_cpus(profile, cores, slice_cpus) * 100.0));
    let weight = format!("CPUWeight={NATIVE_CPU_WEIGHT}");
    let io = format!("IOWeight={NATIVE_CPU_WEIGHT}");
    let isolated = env("AGENT_CAPACITY_SUBSLICE").is_some_and(|name| !name.is_empty());
    if !isolated {
        for (slice, weight) in desktop_weights(profile) {
            set_slice(slice, &[&format!("CPUWeight={weight}"), &format!("IOWeight={weight}")]);
        }
    }
    set_slice(&aggregate_slice(), &[&quota]) && set_slice(&batch_slice(), &BATCH_PROPERTIES) && set_slice(&native_slice(), &[&weight, &io])
}

pub(super) const BATCH_PROPERTIES: [&str; 2] = ["CPUWeight=idle", "CPUQuota="];

pub fn scope_unit(id: &str) -> String {
    format!("agent-capacity-{}.scope", id.replace('-', ""))
}

pub fn scope_live(id: &str) -> bool {
    let state = systemctl(false, &["show", &scope_unit(id), "--property=ActiveState", "--value"])
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    // Unknown manager state must not free a possibly live resource allowance.
    !matches!(state.as_str(), "inactive" | "failed")
}

pub fn kill_scope(id: &str) -> bool {
    succeeded(systemctl(false, &["kill", "--signal=SIGKILL", &scope_unit(id)]))
}

pub fn stop_scope(unit: &str) {
    let _ = systemctl(false, &["stop", unit]);
}
