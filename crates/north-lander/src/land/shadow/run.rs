use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use super::super::candidate::{self, Pick};
use super::super::checks::{ROUND, SCRUBBED};
use super::super::queue::Ticket;
use super::super::setting;
use super::super::slots::select::Config;
use super::super::slots::unit_reuse;
use super::super::slots::graph_selection::Away;
use super::super::slots::verdict_key;
use super::check_phase::{Phase, ShadowCheck};
use super::{OWNER, Settings};
use north_foundation::foundation::clock::Timestamp;
use north_foundation::foundation::exit::{NO_VERDICT_STATUS, RED_MAIN_STATUS, SLOTS_BUSY_STATUS, TIMED_OUT_STATUS};
use north_foundation::foundation::offload::OWNER_ENV;
use north_base::lifecycle::{Owner, Refusal, Store};
use north_offload::check_offload::{detached_scope, launch_detached, pid_file};


const PID_GRACE: Duration = Duration::from_secs(600);

pub struct Found {
    pub head: String,
    pub lines: Vec<(String, String)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
}

fn north(settings: &Settings, worktree: &Path, arguments: &[String]) -> Option<std::process::Output> {
    let (program, prefix) = settings.north.split_first()?;
    Command::new(program).args(prefix).args(arguments).current_dir(worktree).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()
}

fn failed_marker(logs: &Path, key: &str) -> PathBuf {
    logs.join(format!("{key}.failed"))
}

pub fn missing(settings: &Settings, worktree: &Path, main: &str, host: &str, path: &Path, ticket: &Ticket, logs: &Path) -> Option<Found> {
    let built = candidate::build(worktree, main, &[(path.to_owned(), ticket.clone())], &[], 1, logs).ok()?;
    if !matches!(built.picks.as_slice(), [Pick::Included]) {
        return None;
    }
    let commands = Config::load(worktree, &built.head).commands(worktree, main, &built.head).ok()?;
    let program = Path::new(settings.north.first()?);
    let environment = verdict_key::lane_environment(worktree, main, &built.head, Away::on(program, host, main, &built.head).as_ref(), &commands);
    let keyed: Vec<(String, String)> = commands.into_iter().filter_map(|line| Some((line.clone(), verdict_key::before_landing_key(worktree, main, &built.head, &line, &environment)?))).collect();
    if keyed.is_empty() {
        return None;
    }
    let keys: Vec<&str> = keyed.iter().map(|(_, key)| key.as_str()).collect();
    let output = north(settings, worktree, &verdict_key::hit_arguments(&keys)).filter(|output| output.status.success())?;
    let held = verdict_key::hits(&String::from_utf8_lossy(&output.stdout));
    let lines: Vec<(String, String)> = keyed.into_iter().filter(|(_, key)| !held.contains_key(key) && !failed_marker(logs, key).exists()).collect();
    (!lines.is_empty()).then_some(Found { head: built.head, lines })
}

pub fn verdict_of(code: Option<i32>) -> Option<Verdict> {
    match code {
        Some(0) => Some(Verdict::Pass),
        Some(NO_VERDICT_STATUS | RED_MAIN_STATUS | SLOTS_BUSY_STATUS | TIMED_OUT_STATUS) | None => None,
        Some(signal) if signal > 128 => None,
        Some(_) => Some(Verdict::Fail),
    }
}

fn wrapped(capacity: &str, argv: Vec<String>) -> Vec<String> {
    if capacity.is_empty() {
        return argv;
    }
    let mut command = vec!["bash".to_owned(), "-euo".to_owned(), "pipefail".to_owned(), "-c".to_owned(), format!("exec {capacity} \"$@\""), "north-land-shadow-capacity".to_owned()];
    command.extend(argv);
    command
}

fn marker_of(logs: &Path, key: &str, kind: &str) -> PathBuf {
    logs.join(format!("{key}.{kind}"))
}

fn exit_code(exit: &Path) -> Option<i32> {
    fs::read_to_string(exit).ok()?.trim().parse().ok()
}

fn launched_on(cause: &str) -> Option<(String, String)> {
    let mut words = cause.split_whitespace();
    Some((words.next()?.to_owned(), words.next()?.to_owned()))
}

fn store(logs: &Path) -> Option<Store> {
    fs::create_dir_all(logs).ok()?;
    Store::open(&logs.join("shadow-checks.db")).ok()
}

fn holder_of(exit: &Path) -> Option<Owner> {
    for _ in 0..200 {
        if pid_file(exit).exists() {
            return wrapper_of(exit);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

fn record(settings: &Settings, worktree: &Path, logs: &Path, key: &str, host: &str, head: &str, code: i32) -> bool {
    let Some(verdict) = verdict_of(Some(code)) else { return false };
    let passed = (verdict == Verdict::Pass).then_some(head);
    if verdict == Verdict::Fail {
        let _ = fs::write(failed_marker(logs, key), "");
    }
    north(settings, worktree, &verdict_key::record_arguments(host, key, passed)).is_some_and(|output| output.status.success())
}

struct Queued<'a> {
    main: &'a str,
    host: &'a str,
    head: &'a str,
    line: &'a str,
    key: &'a str,
}

fn start(settings: &Settings, worktree: &Path, logs: &Path, capacity: &str, scope: bool, queued: &Queued) -> bool {
    let Queued { main, host, head, line, key } = *queued;
    let mut argv = settings.checker.clone();
    argv.extend(["--leased", "--base", main, "--head", head, "--runner-host", host, "--command", line].map(str::to_owned));
    let argv = wrapped(capacity, argv);
    let Ok(log) = File::create(logs.join(format!("{key}.log"))) else { return false };
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).current_dir(worktree).stdin(Stdio::null()).env("NORTH_LAND_SERVING", "1").env(unit_reuse::RECORD_ENV, "1").env(OWNER_ENV, OWNER).env(ROUND, OWNER);
    for name in SCRUBBED {
        command.env_remove(name);
    }
    let exit = marker_of(logs, key, "exit");
    let _ = fs::remove_file(&exit);
    let _ = fs::remove_file(pid_file(&exit));
    if !launch_detached(&command, &log, &exit, scope) {
        return false;
    }
    let holder = holder_of(&exit).unwrap_or_else(Owner::current);
    let Some(mut store) = store(logs) else { return false };
    store.transition_for::<ShadowCheck>(key, Phase::Launched, &format!("{host} {head}"), Timestamp::now(), &holder).is_ok_and(|moved| moved.is_ok())
}

pub struct Launched {
    pub started: usize,
    pub failed: usize,
}

pub fn check(settings: &Settings, worktree: &Path, main: &str, host: &str, found: &Found, logs: &Path) -> Launched {
    let capacity = setting::landing(&settings.repo, "refs/remotes/origin/main", "landing.capacity");
    let scope = detached_scope();
    let _ = fs::create_dir_all(logs);
    let mut launched = Launched { started: 0, failed: 0 };
    for (line, key) in &found.lines {
        let queued = Queued { main, host, head: &found.head, line, key };
        if start(settings, worktree, logs, &capacity, scope, &queued) {
            launched.started += 1;
        } else {
            launched.failed += 1;
        }
    }
    launched
}

pub struct Harvest {
    pub recorded: usize,
    pub live: usize,
}

pub fn harvest(settings: &Settings, worktree: &Path, logs: &Path) -> Harvest {
    let mut harvest = Harvest { recorded: 0, live: 0 };
    let Some(mut store) = store(logs) else { return harvest };
    let Ok(records) = store.instances::<ShadowCheck>() else { return harvest };
    for row in records.into_iter().filter(|row| row.state == Phase::Launched) {
        match settle(&mut store, settings, worktree, logs, &row.instance, &row.cause) {
            Settled::Recorded => harvest.recorded += 1,
            Settled::Live => harvest.live += 1,
            Settled::Idle => {}
        }
    }
    harvest
}

enum Settled {
    Recorded,
    Live,
    Idle,
}

fn settle(store: &mut Store, settings: &Settings, worktree: &Path, logs: &Path, instance: &str, cause: &str) -> Settled {
    let Some((host, head)) = launched_on(cause) else { return Settled::Idle };
    match store.transition::<ShadowCheck>(instance, Phase::Recorded, "harvested", Timestamp::now()) {
        Ok(Ok(_)) => {}
        Ok(Err(Refusal::Held { .. })) => return Settled::Live,
        _ => return Settled::Idle,
    }
    let exit = marker_of(logs, instance, "exit");
    if let Some(code) = exit_code(&exit) {
        let recorded = record(settings, worktree, logs, instance, &host, &head, code);
        remove_markers(&exit);
        return if recorded { Settled::Recorded } else { Settled::Idle };
    }
    match unfinished_holder(logs, instance) {
        Some(owner) if store.transition_for::<ShadowCheck>(instance, Phase::Launched, cause, Timestamp::now(), &owner).is_ok_and(|moved| moved.is_ok()) => Settled::Live,
        Some(_) => Settled::Idle,
        None => {
            remove_markers(&exit);
            Settled::Idle
        }
    }
}

fn remove_markers(exit: &Path) {
    let _ = fs::remove_file(exit);
    let _ = fs::remove_file(pid_file(exit));
}

fn unfinished_holder(logs: &Path, instance: &str) -> Option<Owner> {
    let exit = marker_of(logs, instance, "exit");
    if pid_file(&exit).exists() {
        return wrapper_of(&exit);
    }
    let launched_at = fs::metadata(marker_of(logs, instance, "log")).and_then(|meta| meta.modified()).ok();
    let overdue = launched_at.is_none_or(|at| SystemTime::now().duration_since(at).is_ok_and(|age| age > PID_GRACE));
    (!overdue).then(Owner::current)
}

fn wrapper_of(exit: &Path) -> Option<Owner> {
    let pid = fs::read_to_string(pid_file(exit)).ok()?.trim().parse::<u32>().ok()?;
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let argument = exit.to_string_lossy();
    cmdline.split(|byte| *byte == 0).any(|arg| arg == argument.as_bytes()).then(|| Owner::of(pid)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use north_base::platform::wait;
    use std::time::Duration;

    #[test]
    fn a_queued_check_line_starts_detached_and_is_recorded_once_it_ends() {
        let world = tempfile::tempdir().unwrap();
        let settings = Settings {
            repo: world.path().to_owned(),
            state: world.path().to_owned(),
            north: vec!["true".to_owned()],
            queue: world.path().to_owned(),
            host: "local".to_owned(),
            checker: vec!["sh".to_owned(), "-c".to_owned(), "sleep 2".to_owned()],
            chooser: Vec::new(),
        };
        let found = Found { head: "a".repeat(40), lines: vec![("cargo nextest run".to_owned(), "line-key".to_owned())] };
        let logs = world.path().join("logs");
        let launched = check(&settings, world.path(), "main", "gex45", &found, &logs);
        let exit = marker_of(&logs, "line-key", "exit");
        assert!(!exit.exists(), "the check ran in the foreground");
        assert_eq!((launched.started, launched.failed), (1, 0));
        assert!(wait::until(Duration::from_secs(10), || pid_file(&exit).exists()));
        assert_eq!(harvest(&settings, world.path(), &logs).live, 1);
        assert!(wait::until(Duration::from_secs(10), || exit.exists()));
        let mut recorded = 0;
        assert!(wait::until(Duration::from_secs(10), || {
            recorded += harvest(&settings, world.path(), &logs).recorded;
            recorded > 0
        }));
        assert_eq!(recorded, 1);
    }
}
