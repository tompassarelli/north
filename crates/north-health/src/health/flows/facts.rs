//! `north health facts`: this host's own record of each flow's outcome, as JSON for the laptop's watchdog.
use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use north_base::agents_sync::UNPUBLISHED;
use north_base::dispatch_pause::{RECORD, TIMER};
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

fn spool_since(spool: &Path) -> Option<f64> {
    entries(spool).filter(|path| path.extension().is_none_or(|extension| extension != "failed")).filter_map(|path| modified(&path)).reduce(f64::min)
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

fn lifecycles() -> Lifecycles {
    match Store::host().and_then(|store| store.faults(Timestamp::now())) {
        Ok(faults) => Lifecycles::Read(faults),
        Err(error) => Lifecycles::Unreadable(error.to_string()),
    }
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
        newest_activity: newest_activity(&state.join("fleet")),
        spool_since: spool_since(&home.join(".local/state/threads/spool")),
        lifecycles: lifecycles(),
    }
}
