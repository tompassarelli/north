use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::fail;
use super::json::{Obj, field, num, text};
use super::machine::{env, now_ms, process_start, read_signals, read_text, uid, wrapper_identity};
use super::policy::gpu_busy_observation;
use super::systemd::{apply_profile, kill_scope, scope_live};

const LOCK_PATIENCE_MILLISECONDS: u32 = 60_000;
pub(crate) const RUN_LEASE_GRACE_MILLISECONDS: f64 = 5000.0;
const NATIVE_WAITING_MILLISECONDS: f64 = 60_000.0;

pub(crate) fn runtime_root() -> PathBuf {
    let base = env("XDG_RUNTIME_DIR").unwrap_or_else(|| format!("/run/user/{}", uid()));
    if !base.starts_with('/') {
        fail("XDG_RUNTIME_DIR must be absolute", 2);
    }
    Path::new(&base).join("agent-capacity-v1")
}

pub(crate) fn mode_profile(mode: &str) -> Option<&'static str> {
    match mode {
        "present" => Some("attended"),
        "away" => Some("unattended"),
        _ => None,
    }
}

pub(crate) fn known_mode(mode: &str) -> bool {
    matches!(mode, "present" | "away" | "auto")
}

/// The chosen mode and the profile it applies; auto is attended.
pub(crate) fn active_profile(root: &Path) -> (String, &'static str) {
    let mode = read_text(root.join("mode")).map(|text| text.trim().to_owned()).filter(|mode| known_mode(mode)).unwrap_or_else(|| "auto".to_owned());
    let profile = mode_profile(&mode).unwrap_or("attended");
    (mode, profile)
}

pub(crate) fn write_private(path: &Path, contents: &str, create_new: bool) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).mode(0o600);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    options.open(path)?.write_all(contents.as_bytes())
}

fn or_fail<T>(result: std::io::Result<T>, what: &Path) -> T {
    result.unwrap_or_else(|error| fail(&format!("{}: {error}", what.display()), 1))
}

pub(crate) fn ensure_state(root: &Path) {
    for directory in ["leases", "queue"] {
        let path = root.join(directory);
        or_fail(fs::DirBuilder::new().recursive(true).mode(0o700).create(&path), &path);
    }
}

pub(crate) fn ensure_profile(root: &Path, force: bool) -> bool {
    let (_, profile) = active_profile(root);
    let marker = root.join("applied-profile");
    if !force && read_text(&marker).as_deref() == Some(profile) {
        return true;
    }
    ensure_state(root);
    if !apply_profile(profile, read_signals().cores) {
        return false;
    }
    or_fail(write_private(&marker, profile, false), &marker);
    true
}

/// Holds the shared admission lock, an exclusive SQLite transaction on admission.lock, while the action runs.
pub(crate) fn with_lock<T>(root: &Path, action: impl FnOnce() -> T) -> T {
    ensure_state(root);
    let path = root.join("admission.lock");
    let lock = rusqlite::Connection::open(&path).unwrap_or_else(|error| fail(&format!("{}: {error}", path.display()), 1));
    let begun = lock
        .busy_timeout(std::time::Duration::from_millis(LOCK_PATIENCE_MILLISECONDS.into()))
        .and_then(|()| lock.execute_batch("BEGIN EXCLUSIVE"));
    match begun {
        Ok(()) => {}
        Err(rusqlite::Error::SqliteFailure(error, _)) if error.code == rusqlite::ErrorCode::DatabaseBusy => fail("shared admission lock remained busy", 75),
        Err(error) => fail(&format!("{}: {error}", path.display()), 1),
    }
    let result = action();
    drop(lock);
    result
}

pub(crate) fn lease_path(root: &Path, id: &str) -> PathBuf {
    root.join("leases").join(format!("{id}.json"))
}

pub(crate) fn exact_lease(root: &Path, id: &str) -> PathBuf {
    if id.len() != 36 || !id.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || byte == b'-') {
        fail("invalid lease identity", 2);
    }
    lease_path(root, id)
}

pub(crate) fn remove_lease(path: &Path) {
    or_fail(fs::remove_file(path), path);
    let _ = fs::remove_file(path.with_extension("usage"));
}

pub(crate) fn read_usage(root: &Path, id: &str) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(root.join("leases").join(format!("{id}.usage"))).ok()?).ok()
}

fn sorted_names(directory: &Path) -> Vec<String> {
    let entries = or_fail(fs::read_dir(directory), directory);
    entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()
}

fn lease_expired(lease: &Value, now: f64) -> bool {
    let run = text(lease, "kind") == Some("run");
    let expires = lease.get("expiresAt").and_then(Value::as_i64);
    let frozen = lease.get("frozenAt").is_some_and(|frozen| !frozen.is_null());
    let overdue = expires.map_or(!run, |expires| expires as f64 <= now && !frozen);
    let id = text(lease, "id").unwrap_or_default();
    if !run {
        return overdue;
    }
    if overdue {
        return kill_scope(id) || !scope_live(id);
    }
    let wrapper_gone = match text(lease, "wrapperStart") {
        None => now - field(lease, "createdAt").unwrap_or(f64::NAN) > RUN_LEASE_GRACE_MILLISECONDS,
        Some(start) => process_start(&field(lease, "wrapperPid").map_or_else(String::new, |pid| pid.to_string())).as_deref() != Some(start),
    };
    wrapper_gone && !scope_live(id)
}

/// The live leases, after deleting each expired one.
pub(crate) fn read_leases(root: &Path, now: f64) -> (Vec<Value>, u32) {
    let directory = root.join("leases");
    let mut active = Vec::new();
    let mut reclaimed = 0;
    for name in sorted_names(&directory).into_iter().filter(|name| name.ends_with(".json")) {
        let path = directory.join(&name);
        let lease: Value = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| fail(&format!("malformed helper-owned lease: {}", path.display()), 2));
        if lease_expired(&lease, now) {
            remove_lease(&path);
            reclaimed += 1;
        } else {
            active.push(lease);
        }
    }
    (active, reclaimed)
}

pub(crate) fn write_lease(path: &Path, lease: &Obj, create_new: bool) {
    or_fail(write_private(path, &format!("{}\n", lease.text()), create_new), path);
}

/// Queues a held wrapper's ticket; names sort by arrival.
pub(crate) fn enqueue(root: &Path, ticket: Obj) -> String {
    ensure_state(root);
    let name = format!("{:015}-{}.json", now_ms() as u64, super::machine::uuid());
    let (pid, start) = wrapper_identity();
    let entry = Obj::new().put("wrapperPid", pid).put("wrapperStart", start).merge(ticket);
    let path = root.join("queue").join(&name);
    or_fail(write_private(&path, &format!("{}\n", entry.text()), true), &path);
    name
}

pub(crate) fn dequeue(root: &Path, ticket: &str) {
    let _ = fs::remove_file(root.join("queue").join(ticket));
}

/// Waiting tickets in arrival order; a ticket whose wrapper died is dropped.
pub(crate) fn read_queue(root: &Path) -> Vec<Value> {
    let directory = root.join("queue");
    let mut names = sorted_names(&directory);
    names.sort();
    let mut waiting = Vec::new();
    for name in names.into_iter().filter(|name| name.ends_with(".json")) {
        let Some(Value::Object(mut ticket)) = fs::read_to_string(directory.join(&name)).ok().and_then(|text| serde_json::from_str(&text).ok()) else {
            dequeue(root, &name);
            continue;
        };
        let pid = ticket.get("wrapperPid").map_or_else(String::new, |pid| pid.to_string());
        if process_start(&pid).as_deref() != ticket.get("wrapperStart").and_then(Value::as_str) {
            dequeue(root, &name);
            continue;
        }
        ticket.insert("queuedAt".to_owned(), num(super::json::number(&name[..15.min(name.len())])));
        ticket.insert("name".to_owned(), Value::String(name));
        waiting.push(Value::Object(ticket));
    }
    waiting
}

/// Admission order: critical and tom, then exclusive, then the rest, each part in arrival order.
pub(crate) fn priority_order(queue: Vec<Value>) -> Vec<Value> {
    let rank = |ticket: &Value| match text(ticket, "class") {
        Some("critical" | "tom") => 0,
        Some("exclusive") => 1,
        _ => 2,
    };
    let mut ordered = queue;
    ordered.sort_by_key(rank);
    ordered
}

pub(crate) fn native_waiting(root: &Path, now: f64) -> u32 {
    let modified = fs::metadata(root.join("native-waiting")).and_then(|metadata| metadata.modified());
    match modified {
        Ok(modified) => {
            let at = modified.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64() * 1000.0);
            u32::from(now - at < NATIVE_WAITING_MILLISECONDS)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => fail(&format!("native-waiting: {error}"), 1),
    }
}

pub(crate) fn mark_native_waiting(root: &Path, waiting: bool) {
    let path = root.join("native-waiting");
    if waiting {
        or_fail(write_private(&path, "", false), &path);
    } else {
        let _ = fs::remove_file(path);
    }
}

/// Records when the GPU first crossed the busy limit, and reports the streak as of now.
pub(crate) fn record_gpu_busy(root: &Path, busy_percent: f64, first_sample_at: f64) -> Obj {
    let path = root.join("gpu-busy-since");
    let previous = read_text(&path).map_or(0.0, |text| super::json::number(&text));
    let (since, _) = gpu_busy_observation(first_sample_at, busy_percent, (previous > 0.0).then_some(previous));
    match since {
        None => drop(fs::remove_file(&path)),
        Some(since) => or_fail(write_private(&path, &super::json::show(since), false), &path),
    }
    let (since, seconds) = gpu_busy_observation(now_ms(), busy_percent, since);
    Obj::new().put("gpuBusySince", super::json::opt(since)).put("gpuBusySeconds", num(seconds))
}
