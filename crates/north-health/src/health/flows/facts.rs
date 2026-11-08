//! `north health facts`: this host's own record of each flow's outcome, as JSON for the laptop's watchdog.
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use north_base::agents_sync::UNPUBLISHED;
use north_base::dispatch_pause::{RECORD, TIMER};
use north_base::error::NorthResult;
use north_base::lifecycle::Store;
use north_pace::usage::profiles;

use super::{Dispatch, Held, HostFacts, Lifecycles};
use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};

fn modified(path: &Path) -> Option<f64> {
    Some(fs::symlink_metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn held(path: &Path) -> Option<Held> {
    let cause = fs::read_to_string(path).ok()?;
    Some(Held { since: modified(path)?, cause: cause.trim().to_owned() })
}

fn entries(directory: &Path) -> impl Iterator<Item = std::path::PathBuf> + use<> {
    fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path())
}

/// The newest write to any fleet session's stream on this host: a worker was running then.
fn newest_activity(fleet: &Path) -> Option<f64> {
    entries(fleet)
        .filter(|session| session.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.')))
        .flat_map(|session| entries(&session))
        .filter_map(|generation| modified(&generation.join("stream.jsonl")))
        .reduce(f64::max)
}


/// The ledger host keeps threads.db and no `remote` pointer; only it can say when the newest run was recorded.
fn newest_run(home: &Path) -> Option<f64> {
    if home.join(".config/threads/remote").exists() {
        return None;
    }
    let rows = sqlite::rows(&home.join(".local/state/threads/threads.db"), Access::ReadOnly, Duration::from_secs(5), "select max(ended) as ended from runs").ok()?;
    let ended = rows.first()?.get("ended")?.as_str()?.to_owned();
    Timestamp::parse_rfc3339(&ended).map(Timestamp::unix_seconds_f64)
}

/// The ledger host's backstop timer's interval, as systemd shows it in `OnUnitActiveUSec=1h 30min`.
fn recording_interval() -> Option<f64> {
    let shown = systemctl(&["--user", "show", LEDGER_TIMER, "-p", "TimersMonotonic"]);
    let span = shown.split("OnUnitActiveUSec=").nth(1)?.split([';', '}']).next()?;
    span.split_whitespace().map(span_seconds).sum::<Option<f64>>().filter(|seconds| *seconds > 0.0)
}

fn span_seconds(part: &str) -> Option<f64> {
    let split = part.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let amount: f64 = part[..split].parse().ok()?;
    let unit = match &part[split..] {
        "us" => 1e-6,
        "ms" => 1e-3,
        "s" => 1.0,
        "min" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        "w" => 604_800.0,
        _ => return None,
    };
    Some(amount * unit)
}

fn systemctl(arguments: &[&str]) -> String {
    super::super::command("systemctl", arguments, None, 5).unwrap_or_default()
}

fn property<'a>(shown: &'a str, key: &str) -> Option<&'a str> {
    shown.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

fn dispatch(state: &Path) -> Option<Dispatch> {
    let timer = systemctl(&["--user", "show", TIMER, "-p", "LoadState", "-p", "ActiveState"]);
    if property(&timer, "LoadState") != Some("loaded") {
        return None;
    }
    let service = TIMER.replace(".timer", ".service");
    let exited = systemctl(&["--user", "show", &service, "-p", "ExecMainExitTimestamp", "--timestamp=unix"]);
    let last_run = property(&exited, "ExecMainExitTimestamp").and_then(|at| at.strip_prefix('@')?.parse().ok());
    Some(Dispatch { timer_active: property(&timer, "ActiveState") == Some("active"), paused: held(&state.join(RECORD)), last_run })
}

const LEDGER_TIMER: &str = "worker-ledger.timer";

const CAPACITY_LEASES: &str = "agent-capacity-v1/lifecycle.db";

/// The stores of lifecycle records on this host: the run ledger, and the capacity runtime directory's, which lives and dies with the lease files beside it.
fn stores(home: &Path) -> Vec<NorthResult<Store>> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).or_else(|| Some(format!("/run/user/{}", fs::metadata(home).ok()?.uid()).into()));
    let leases = runtime.map(|runtime| runtime.join(CAPACITY_LEASES)).filter(|path| path.exists());
    std::iter::once(Store::host()).chain(leases.map(|path| Store::open(&path))).collect()
}

fn lifecycles(home: &Path) -> Lifecycles {
    let now = Timestamp::now();
    let mut faults = Vec::new();
    for store in stores(home) {
        match store.and_then(|store| store.faults(now)) {
            Ok(found) => faults.extend(found),
            Err(error) => return Lifecycles::Unreadable(error.to_string()),
        }
    }
    Lifecycles::Read(faults)
}

pub fn gather(home: &Path) -> HostFacts {
    let state = home.join(".local/state/north");
    let runtime = fs::read_link(state.join("current")).ok().and_then(|link| link.file_name().map(|name| name.to_string_lossy().into_owned()));
    let policy = profiles::policy_value(&crate::platform::orchestration::path());
    HostFacts {
        host: crate::platform::host::name(),
        runtime,
        policy_held: held(&state.join(UNPUBLISHED)),
        policy_published: modified(&state.join("agents/current")),
        unhooked: profiles::unhooked(&policy, &crate::platform::host::name(), home),
        dispatch: dispatch(&state),
        newest_run: newest_run(home),
        recording_interval_s: recording_interval(),
        newest_activity: newest_activity(&state.join("fleet")),
        lifecycles: lifecycles(home),
    }
}
