use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::Ctx;

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

pub(super) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Number(number)) => number.as_f64() != Some(0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

fn or_unknown(value: &Value) -> String {
    if truthy(Some(value)) { py_str(value) } else { "unknown".into() }
}

pub(super) fn agent_key(event: &Value) -> String {
    let session = or_unknown(&event["session_id"]);
    if truthy(event.get("agent_id")) { format!("{session}--{}", py_str(&event["agent_id"])) } else { session }
}

pub(super) fn state_path(ctx: &Ctx, session: &str) -> PathBuf {
    let safe: String = session
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') { c } else { '_' })
        .collect();
    ctx.state_root.join(format!("{safe}.json"))
}

fn locked(path: &Path) -> Option<File> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    let lock = File::create(path.with_extension("lock")).ok()?;
    lock.lock().ok()?;
    Some(lock)
}

fn save(path: &Path, data: &Value) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, data.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub(super) fn with_state<R>(ctx: &Ctx, key: &str, body: impl FnOnce(&mut Map<String, Value>) -> R) -> R {
    let path = state_path(ctx, key);
    let lock = locked(&path);
    let raw = lock
        .as_ref()
        .and_then(|_| std::fs::read(&path).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let mut data = match raw {
        Some(Value::Object(map)) => map,
        Some(Value::Array(prompts)) => Map::from_iter([("prompts".to_owned(), Value::Array(prompts))]),
        _ => Map::new(),
    };
    data.entry("prompts").or_insert_with(|| json!([]));
    data.entry("started").or_insert_with(|| json!(ctx.now));
    data.entry("opened").or_insert_with(|| json!(0));
    data.entry("closed").or_insert_with(|| json!(0));
    data.entry("passed").or_insert_with(|| json!({}));
    let result = body(&mut data);
    save(&path, &Value::Object(data));
    drop(lock);
    result
}

pub(super) fn with_shared<R>(ctx: &Ctx, body: impl FnOnce(&mut Map<String, Value>) -> R) -> R {
    let path = ctx.state_root.join("passed-by-commit.json");
    let lock = locked(&path);
    let mut data = match lock.as_ref().and_then(|_| std::fs::read(&path).ok()).and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let result = body(&mut data);
    save(&path, &Value::Object(data));
    drop(lock);
    result
}

pub(super) fn prompts(state: &Map<String, Value>, last: usize) -> Vec<String> {
    let all: Vec<String> = state["prompts"]
        .as_array()
        .map(|items| items.iter().map(py_str).collect())
        .unwrap_or_default();
    all[all.len().saturating_sub(last)..].to_vec()
}

pub(super) fn count(value: Option<&Value>) -> i64 {
    value.and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(0)
}

pub(super) fn tree_counts(ctx: &Ctx, event: &Value, state: &Map<String, Value>) -> (i64, i64) {
    let (mut opened, mut closed) = (count(state.get("opened")), count(state.get("closed")));
    let own = state_path(ctx, &agent_key(event));
    let root = state_path(ctx, &or_unknown(&event["session_id"]));
    let stem = root.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut paths = vec![root];
    let mut workers: Vec<PathBuf> = std::fs::read_dir(&ctx.state_root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            name.starts_with(&format!("{stem}--")) && name.ends_with(".json")
        })
        .collect();
    workers.sort();
    paths.extend(workers);
    for path in paths.into_iter().filter(|path| *path != own) {
        let Some(Value::Object(other)) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()) else {
            continue;
        };
        opened += count(other.get("opened"));
        closed += count(other.get("closed"));
    }
    (opened, closed)
}

pub(super) struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
}

pub(super) fn run_within(program: &str, arguments: &[&str], seconds: u64) -> Option<Output> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    Some(Output { success: status.success(), stdout: reader.join().ok()? })
}

fn git(cwd: &str, arguments: &[&str]) -> Option<Output> {
    let mut all = vec!["-C", cwd];
    all.extend_from_slice(arguments);
    run_within("git", &all, 5)
}

fn git_checked(cwd: &str, arguments: &[&str]) -> Option<Vec<u8>> {
    git(cwd, arguments).filter(|out| out.success).map(|out| out.stdout)
}

pub(super) fn head_commit(cwd: &str) -> Option<String> {
    let status = git_checked(cwd, &["status", "--porcelain=v1", "-uno"])?;
    if !String::from_utf8_lossy(&status).trim().is_empty() {
        return None;
    }
    let head = String::from_utf8_lossy(&git_checked(cwd, &["rev-parse", "HEAD"])?).trim().to_owned();
    (!head.is_empty()).then_some(head)
}

pub(super) fn tree_state(cwd: &str) -> Option<String> {
    let head = git_checked(cwd, &["rev-parse", "HEAD"])?;
    let status = git_checked(cwd, &["status", "--porcelain=v1", "-uall"])?;
    let diff = git_checked(cwd, &["diff", "HEAD", "--no-ext-diff"])?;
    Some(sha256_hex(&[head, status, diff].join(&b"\0"[..])))
}

pub(super) fn unlanded(cwd: &str) -> bool {
    let Some(status) = git(cwd, &["status", "--porcelain=v1", "-uno"]) else {
        return false;
    };
    if !String::from_utf8_lossy(&status.stdout).trim().is_empty() {
        return true;
    }
    let Some(ahead) = git(cwd, &["rev-list", "--count", "origin/main..HEAD"]) else {
        return false;
    };
    let count = String::from_utf8_lossy(&ahead.stdout).trim().to_owned();
    ahead.success && !count.is_empty() && count != "0"
}

pub(super) fn run_conclusion(run_id: &str) -> String {
    run_within("gh", &["run", "view", run_id, "--json", "conclusion", "-q", ".conclusion"], 6)
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}

pub(super) fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out = out.canonicalize().unwrap_or(out);
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    let mut existing = out.clone();
    let mut rest = Vec::new();
    while existing.canonicalize().is_err() {
        let Some(name) = existing.file_name().map(|n| n.to_owned()) else { return out };
        rest.push(name);
        existing.pop();
    }
    let mut real = existing.canonicalize().unwrap_or(existing);
    real.extend(rest.iter().rev());
    real
}

pub(super) fn running_copy(cwd: &str, suite: &str) -> Option<(u32, f64)> {
    let here = resolve(Path::new(cwd)).to_string_lossy().into_owned();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(cmdline) = std::fs::read(format!("/proc/{name}/cmdline")) else { continue };
        let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        if !cmdline.contains(suite) {
            continue;
        }
        let Ok(link) = std::fs::read_link(format!("/proc/{name}/cwd")) else { continue };
        let there = resolve(&link).to_string_lossy().into_owned();
        let Ok(started) = std::fs::metadata(format!("/proc/{name}")).and_then(|m| m.modified()) else { continue };
        let started = started.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        if there.starts_with(&here) || here.starts_with(&there) {
            return name.parse().ok().map(|pid| (pid, started));
        }
    }
    None
}
