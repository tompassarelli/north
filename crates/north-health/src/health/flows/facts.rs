//! `north health facts`: this host's own record of each flow's outcome, as JSON for the laptop's watchdog.
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use north_base::agents_sync::UNPUBLISHED;
use north_base::dispatch_pause::{RECORD, TIMER};
use north_base::error::NorthResult;
use north_base::lifecycle::{Fault, Retire, Store};
use north_pace::usage::profiles;

use super::{Dispatch, Held, HostFacts, Lifecycles};
use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};

fn modified(path: &Path) -> Option<f64> {
    Some(fs::symlink_metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn held(path: &Path) -> Option<Held> {
    let pause = north_base::dispatch_pause::Pause::read(path)?;
    Some(Held { since: modified(path)?, cause: pause.reason })
}

fn entries(directory: &Path) -> impl Iterator<Item = std::path::PathBuf> + use<> {
    fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path())
}

/// The newest write to the stream of a fleet session whose host has exited: a worker finished then, so its run is due in the ledger. A session whose host still runs has no run to record yet.
pub(super) fn newest_finished(fleet: &Path) -> Option<f64> {
    entries(fleet)
        .filter(|session| session.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.')))
        .filter(|session| !host_running(session))
        .flat_map(|session| entries(&session))
        .filter_map(|generation| modified(&generation.join("stream.jsonl")))
        .reduce(f64::max)
}

fn host_running(session: &Path) -> bool {
    let Some(pid) = crate::platform::session_host_files::pid(session) else { return false };
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).unwrap_or_default();
    stat.rsplit_once(')').is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
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
    let deploy_restarts = !north_base::dispatch_pause::held_by(&systemctl(&["--user", "cat", TIMER]));
    Some(Dispatch { timer_active: property(&timer, "ActiveState") == Some("active"), deploy_restarts, paused: held(&state.join(RECORD)), last_run })
}

const LEDGER_TIMER: &str = "worker-ledger.timer";

const CAPACITY_LEASES: &str = "agent-capacity-v1/lifecycle.db";

/// The stores of lifecycle records on this host: the run ledger, the threads spool's, which a sandboxed worker writes beside its spool, and the capacity runtime directory's, which lives and dies with the lease files beside it, opened with `capacity`, since only the outside broker writes it.
fn stores(home: &Path, capacity: fn(&Path) -> NorthResult<Store>) -> Vec<NorthResult<Store>> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).or_else(|| Some(format!("/run/user/{}", fs::metadata(home).ok()?.uid()).into()));
    let leases = runtime.map(|runtime| runtime.join(CAPACITY_LEASES)).filter(|path| path.exists());
    let spool = Some(north_threads::threads::client::spool::records_path(&home.join(".local/state/threads/spool"))).filter(|path| path.exists());
    std::iter::once(Store::host()).chain(spool.map(|path| Store::open(&path))).chain(leases.map(|path| capacity(&path))).collect()
}

/// Retires, in every store of this host, the rows whose holder is a process of this host that is gone. Only this host can
/// judge those holders, so a copy of its ledger elsewhere reports them as orphaned until this runs.
pub fn retire_gone(home: &Path, passes: &[Retire]) -> NorthResult<usize> {
    let now = Timestamp::now();
    let mut moved = 0;
    for store in stores(home, Store::open) {
        moved += store?.retire_every(now, passes)?;
    }
    Ok(moved)
}

pub fn lifecycles(home: &Path) -> Lifecycles {
    let now = Timestamp::now();
    let mut faults = Vec::new();
    for store in stores(home, Store::read_only) {
        match store.and_then(|store| store.faults(now)) {
            Ok(found) => faults.extend(found),
            Err(error) => return Lifecycles::Unreadable(error.to_string()),
        }
    }
    name_spool_verbs(home, &mut faults);
    Lifecycles::Read(faults)
}

fn name_spool_verbs(home: &Path, faults: &mut [Fault]) {
    let spool = home.join(".local/state/threads/spool");
    for fault in faults.iter_mut().filter(|fault| fault.machine == "threads-spool" && fault.state == "refused") {
        let Some(verb) = spool_verb(&spool.join(&fault.instance)) else { continue };
        let canonical = format!("the ledger host will not run `{verb}`");
        if !fault.cause.starts_with(&canonical) {
            fault.cause = format!("{canonical}: {}", fault.cause);
        }
    }
}

fn spool_verb(path: &Path) -> Option<String> {
    let argv: Vec<String> = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    argv.into_iter().next()
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
        newest_finished: newest_finished(&state.join("fleet")),
        lifecycles: lifecycles(home),
    }
}

#[cfg(test)]
mod tests {
    use north_base::lifecycle::FaultKind;

    use super::*;

    fn refused(instance: &str) -> Fault {
        Fault { machine: "threads-spool".into(), instance: instance.into(), state: "refused".into(), since_ms: 0, kind: FaultKind::Overdue { due_ms: 0 }, cause: "the ledger host exited 126".into() }
    }

    fn naming(instance: &str, cause: &str) -> Fault {
        Fault { cause: cause.into(), ..refused(instance) }
    }

    #[test]
    fn a_refused_spool_write_names_the_verb_from_its_own_argv_in_the_health_line() {
        let home = tempfile::tempdir().unwrap();
        let spool = home.path().join(".local/state/threads/spool");
        fs::create_dir_all(&spool).unwrap();
        fs::write(spool.join("a.argv"), r#"["claim", "north#7", "--by", "gex45"]"#).unwrap();
        fs::write(spool.join("b.argv"), r#"["release", "north#7"]"#).unwrap();
        let mut faults = vec![refused("a.argv"), refused("b.argv"), refused("gone.argv"), naming("a.argv", "the ledger host will not run `claim`: remote-serve: refused for gex45")];
        name_spool_verbs(home.path(), &mut faults);
        assert_eq!(faults[0].cause, "the ledger host will not run `claim`: the ledger host exited 126");
        assert_eq!(faults[1].cause, "the ledger host will not run `release`: the ledger host exited 126");
        assert_eq!(faults[2].cause, "the ledger host exited 126", "a record whose write is gone keeps the cause it had");
        assert_eq!(faults[3].cause, "the ledger host will not run `claim`: remote-serve: refused for gex45", "a cause that already names the verb is not prefixed twice");
    }
}
