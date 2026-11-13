//! The scope a lease runs in: its systemd command, its own compiler-cache server and its supervisor.

use std::path::Path;
use std::process::Stdio;

use serde_json::Value;

use super::super::json::{Obj, field, show, text};
use super::super::machine::{env, lease_slice, read_text};
use super::super::overrun::Overrun;
use super::super::request::Request;
use super::super::requeue;
use super::super::state::exact_lease;
use super::super::systemd::{client_environment, scope_unit, stop_scope};
use super::super::terminations::Terminations;
use crate::capacity::dev_shell;
use crate::capacity::subject_start;

pub(super) fn timeout_text(timeout_seconds: Option<u64>) -> String {
    timeout_seconds.map_or_else(|| "null".to_owned(), |seconds| seconds.to_string())
}

const FROZEN_CLASSES: [&str; 4] = ["heavy", "moderate", "exclusive", "gpu"];
const USER_HZ: f64 = 100.0;

/// Whether this process runs inside a fleet session scope, which gets a session unit of its own for its leases.
pub(super) fn fleet_session() -> bool {
    env("NORTH_FLEET_SCOPE").is_some_and(|scope| !scope.is_empty())
}

pub fn north_command() -> String {
    north_base::platform::north_binary::north_binary().to_string_lossy().into_owned()
}

/// The lease's own compiler-cache server socket: sccache forks every compile from its server, so one server shared by every lease ran each lease's compiles in the scope of whichever process started it, outside the lease's CPU quota and measurement. Named by the lease's first uuid group because a Unix socket path holds 107 bytes and the private desktop's runtime directory already spends 68 of them before the file name.
pub(crate) fn cache_server_socket(root: &Path, lease: &str) -> std::path::PathBuf {
    root.join("sccache").join(format!("{}.sock", lease.split('-').next().unwrap_or(lease)))
}

pub(crate) fn scope_command(root: &Path, request: &Request, admitted: &Obj, timeout_seconds: Option<u64>, command: &[String]) -> tokio::process::Command {
    let lease = text(&admitted.value(), "lease").unwrap_or_default().to_owned();
    let unit = scope_unit(&lease);
    let north = north_command();
    let mut arguments: Vec<String> = ["--user", "--scope", "--quiet", "--collect", "--expand-environment=no"].map(str::to_owned).to_vec();
    if let (Some(runtime), Some(bus)) = (env("XDG_RUNTIME_DIR"), env("DBUS_SESSION_BUS_ADDRESS")) {
        arguments.extend([format!("--setenv=XDG_RUNTIME_DIR={runtime}"), format!("--setenv=DBUS_SESSION_BUS_ADDRESS={bus}")]);
    }
    arguments.extend([format!("--unit={unit}"), format!("--slice={}", lease_slice(request.admitted_class()))]);
    let scope = admitted.get("scope").cloned().unwrap_or(Value::Null);
    // A quota-throttled game client stalls for the rest of each period.
    if let Some(cpus) = field(&scope, "cpus").filter(|cpus| cpus.is_finite() && *cpus > 0.0 && request.name != "native") {
        arguments.push(format!("--property=CPUQuota={}%", show(cpus * 100.0)));
    }
    if request.landing {
        arguments.push(format!("--property=CPUWeight={}", crate::capacity::admission::default_mib("land_check_cpu_weight") as u32));
    }
    if request.name != "native" {
        arguments.push(format!("--property=TasksMax={}", super::super::policy::task_cap()));
    }
    arguments.extend([format!("--property=MemoryHigh={}M", show(field(&scope, "memoryMiB").unwrap_or(f64::NAN))), "--property=KillMode=control-group".to_owned(), "--property=IOAccounting=yes".to_owned()]);
    if FROZEN_CLASSES.contains(&request.name.as_str()) {
        arguments.extend(["--".to_owned(), north.clone(), "capacity".to_owned(), "deadline".to_owned(), exact_lease(root, &lease).to_string_lossy().into_owned(), timeout_text(timeout_seconds), "--".to_owned()]);
    } else {
        arguments.extend([format!("--property=RuntimeMaxSec={}", timeout_seconds.map_or_else(|| "infinity".to_owned(), |seconds| format!("{seconds}s"))), "--".to_owned()]);
    }
    let jobs = field(&admitted.value(), "requestedCpus").or_else(|| field(&scope, "cpus")).filter(|cpus| cpus.is_finite() && *cpus > 0.0).map_or(1.0, f64::ceil).max(1.0).to_string();
    if request.name == "native" {
        arguments.extend(dev_shell::bare(&north, command));
    } else {
        let sized = north_base::nix_command::sized(command, &jobs);
        arguments.extend(dev_shell::enter(&north, sized.as_deref().unwrap_or(command)));
    }
    let mut child = if fleet_session() {
        let mut child = std::process::Command::new(&north);
        child.arg("session-unit");
        child
    } else {
        std::process::Command::new(super::super::systemd::program("systemd-run"))
    };
    child.args(arguments).stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit()).env("AGENT_CAPACITY_LEASE", &lease).env(subject_start::ENV, subject_start::witness(root, &lease));
    if let Some(pair) = request.pair {
        child.env("AGENT_CAPACITY_PAIR", pair.to_string());
    }
    if request.name != "native" {
        child.env("CARGO_BUILD_JOBS", &jobs).env("RUST_TEST_THREADS", &jobs).env("NEXTEST_TEST_THREADS", &jobs);
        let socket = cache_server_socket(root, &lease);
        if let Some(directory) = socket.parent() {
            let _ = std::fs::create_dir_all(directory);
        }
        child.env("SCCACHE_SERVER_UDS", socket);
    }
    requeue::forget(&mut child);
    client_environment(&mut child);
    tokio::process::Command::from(child)
}

pub(super) fn children_cpu_seconds() -> f64 {
    let stat = read_text("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.get(stat.rfind(')').map_or(0, |end| end + 2)..).unwrap_or_default().split(' ').collect();
    let ticks = |index: usize| fields.get(index).and_then(|field| field.parse::<f64>().ok()).unwrap_or(0.0);
    (ticks(13) + ticks(14)) / USER_HZ
}

pub(super) async fn supervise(mut child: tokio::process::Child, unit: &str, terminations: &mut Terminations, mut overrun: Option<Overrun>) -> i32 {
    let began = tokio::time::Instant::now();
    loop {
        let due = overrun.as_ref().and_then(Overrun::due);
        tokio::select! {
            () = tokio::time::sleep_until(began + due.unwrap_or_default()), if due.is_some() => {
                if let Some((reader, line)) = overrun.as_mut().and_then(|overrun| overrun.next(began.elapsed())) {
                    let session = overrun.as_ref().map_or_else(String::new, |overrun| overrun.session().to_owned());
                    tokio::spawn(super::super::overrun::deliver(north_command(), session, reader, line));
                }
            }
            status = child.wait() => {
                return status.map_or(1, |status| {
                    use std::os::unix::process::ExitStatusExt;
                    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).unwrap_or(1)
                });
            }
            _ = terminations.next() => stop_scope(unit),
        }
    }
}
