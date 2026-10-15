use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};
use rustix::io::Errno;
use rustix::process::{Pid, test_kill_process};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

pub(crate) type Registry = BTreeMap<String, Map<String, Value>>;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(crate) struct EventRow {
    pub(crate) id: i64,
    pub(crate) time: String,
    pub(crate) kind: String,
    pub(crate) item: Option<String>,
    pub(crate) from_node: String,
    pub(crate) to_node: String,
    pub(crate) repo: Option<String>,
    pub(crate) lane: Option<String>,
    pub(crate) sha: Option<String>,
    pub(crate) body: String,
}

pub(crate) struct Ran {
    pub(crate) code: Option<i32>,
    pub(crate) stderr: String,
}

fn failed(message: impl Into<String>) -> NorthError {
    NorthError::State(message.into())
}

fn text<'a>(node: &'a Map<String, Value>, key: &str) -> &'a str {
    node.get(key).and_then(Value::as_str).unwrap_or("")
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Number(number)) => number.as_f64() != Some(0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

fn shown(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

pub(crate) fn load_registry(org_file: &Path) -> NorthResult<Registry> {
    if let Some(directory) = org_file.parent() {
        fs::create_dir_all(directory)?;
    }
    let mut lock_path = org_file.as_os_str().to_owned();
    lock_path.push(".lock");
    let lock = File::create(lock_path)?;
    flock(&lock, FlockOperation::LockExclusive).map_err(io::Error::from)?;
    let data: Value = match fs::read_to_string(org_file) {
        Ok(content) => serde_json::from_str(&content)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => json!({"version": 1}),
        Err(error) => return Err(error.into()),
    };
    if data.get("version") != Some(&json!(1)) {
        return Err(failed(format!("{} has schema version {}; this tool reads version 1", org_file.display(), shown(data.get("version")))));
    }
    let mut proxy = json!({"id": "proxy", "role": "proxy", "domain": "all", "provider": "claude",
        "session": "Tom's top-level claude", "depth": 0, "budget": 3, "state": "active"});
    if let (Some(fields), Some(overrides)) = (proxy.as_object_mut(), data.get("proxy").and_then(Value::as_object)) {
        fields.extend(overrides.clone());
    }
    let mut registry = Registry::new();
    for node in std::iter::once(&proxy).chain(data.get("nodes").and_then(Value::as_array).into_iter().flatten()) {
        let fields = node.as_object().cloned().unwrap_or_default();
        registry.insert(text(&fields, "id").into(), fields);
    }
    Ok(registry)
}


fn alive(pid: i64) -> bool {
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else { return true };
    !matches!(test_kill_process(pid), Err(Errno::SRCH))
}

pub(crate) fn health(node: &Map<String, Value>) -> String {
    if text(node, "state") == "finished" {
        return "finished".into();
    }
    let dead = truthy(node.get("pid")) && !alive(node.get("pid").and_then(Value::as_i64).unwrap_or(0));
    if truthy(node.get("died")) || dead {
        return format!("DIED while active, restart: agents lead restart {}", shown(node.get("id")));
    }
    "active".into()
}

pub(crate) fn describe(row: &EventRow) -> String {
    format!(
        "EVENT {} {} {} from={} to={} lane={} sha={}\n{}\nReply with agents event ANSWER --from-node {} --to-node {} --answer {} --body <answer>.",
        row.id, row.kind, row.item.as_deref().unwrap_or(""), row.from_node, row.to_node,
        row.lane.as_deref().unwrap_or(""), row.sha.as_deref().unwrap_or(""), row.body, row.to_node, row.from_node, row.id
    )
}

pub(crate) fn append(path: &Path, line: &str, deadline: Option<Instant>) -> NorthResult<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let mut stream = OpenOptions::new().read(true).append(true).create(true).open(path)?;
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "event inbox delivery deadline exhausted").into());
    }
    let operation = if deadline.is_some() { FlockOperation::NonBlockingLockExclusive } else { FlockOperation::LockExclusive };
    match flock(&stream, operation) {
        Err(Errno::WOULDBLOCK) => {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "event inbox is locked; retry on the next supervisor pass").into());
        }
        other => other.map_err(io::Error::from)?,
    }
    let mut existing = Vec::new();
    stream.seek(SeekFrom::Start(0))?;
    stream.read_to_end(&mut existing)?;
    let content = format!("{line}\n").into_bytes();
    if existing.windows(content.len()).any(|window| window == content.as_slice()) {
        return Ok(());
    }
    if existing.last().is_some_and(|last| *last != b'\n') {
        stream.write_all(b"\n")?;
    }
    stream.write_all(&content)?;
    Ok(())
}

pub(crate) fn refusal(registry: &Registry, name: &str) -> Option<String> {
    let node = registry.get(name);
    if name == "proxy" {
        return None;
    }
    let Some(node) = node.filter(|node| !node.is_empty()) else {
        return Some("removed from the org".into());
    };
    if text(node, "state") != "active" {
        return Some(format!("{} in the org", shown(node.get("state"))));
    }
    let content = fs::read_to_string(text(node, "status_file")).unwrap_or_default();
    if content.trim().lines().last().is_some_and(|line| line.starts_with("FINISHED")) {
        return Some(format!("FINISHED in {}", shown(node.get("status_file"))));
    }
    if health(node) != "active" {
        return Some("a dead session".into());
    }
    None
}

pub(crate) fn progressed(row: &EventRow, registry: &Registry) -> NorthResult<bool> {
    let Some(owner) = registry.get(&row.to_node) else { return Ok(false) };
    if owner.is_empty() || refusal(registry, &row.to_node).is_some() {
        return Ok(false);
    }
    let since = Timestamp::parse_python_isoformat(&row.time).ok_or_else(|| failed(format!("Invalid isoformat string: {:?}", row.time)))?;
    if let Ok(modified) = fs::metadata(text(owner, "status_file")).and_then(|meta| meta.modified()) {
        if Timestamp::from(modified) > since {
            return Ok(true);
        }
    }
    for node in registry.values() {
        if node.get("parent") == owner.get("id") && Timestamp::parse_python_isoformat_seconds(text(node, "started")).is_some_and(|started| started >= since) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn inbox_path(node: &Map<String, Value>, org_file: &Path) -> PathBuf {
    if text(node, "id") == "proxy" {
        return org_file.parent().unwrap_or(Path::new("")).join("handoffs/proxy-inbox.md");
    }
    let status = Path::new(text(node, "status_file"));
    let name = status.file_name().and_then(|name| name.to_str()).unwrap_or("");
    status.with_file_name(format!("{}-inbox.md", name.strip_suffix("-status.md").unwrap_or(name)))
}

fn inbox(node: &Map<String, Value>, org_file: &Path, line: &str, deadline: Option<Instant>) -> NorthResult<()> {
    if text(node, "host") == "north" {
        return Ok(());
    }
    append(&inbox_path(node, org_file), line, deadline)
}

pub(crate) fn mirror(row: &EventRow, registry: &Registry, org_file: &Path, deadline: Option<Instant>) -> NorthResult<()> {
    let empty = Map::new();
    let source = registry.get(&row.from_node).unwrap_or(&empty);
    let target = registry.get(&row.to_node).unwrap_or(&empty);
    let line = describe(row);
    if truthy(source.get("status_file")) {
        append(Path::new(text(source, "status_file")), &line, deadline)?;
    }
    if row.to_node == "proxy" {
        let mut proxy = registry.get("proxy").cloned().unwrap_or_default();
        proxy.insert("id".into(), json!("proxy"));
        inbox(&proxy, org_file, &line, deadline)?;
    } else if truthy(target.get("status_file")) {
        inbox(target, org_file, &line, deadline)?;
    }
    Ok(())
}

pub(crate) fn run(argv: &[String], env: &[(&str, &str)], timeout: Duration, deadline: Option<Instant>) -> NorthResult<Ran> {
    let timeout = match deadline {
        Some(deadline) => {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "landing-health total deadline exhausted").into());
            }
            timeout.min(left)
        }
        None => timeout,
    };
    let (program, arguments) = argv.split_first().ok_or_else(|| failed("empty command"))?;
    let mut child = Command::new(program).args(arguments).envs(env.iter().copied())
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let drain = thread::spawn(move || {
        let mut sink = Vec::new();
        if let Some(stdout) = stdout.as_mut() { let _ = stdout.read_to_end(&mut sink); }
    });
    let errors = thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() { let _ = stderr.read_to_string(&mut text); }
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, format!("Command {argv:?} timed out after {} seconds", timeout.as_secs_f64())).into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let _ = drain.join();
    Ok(Ran { code: status.code(), stderr: errors.join().unwrap_or_default() })
}

fn check(ran: Ran, fallback: &str) -> NorthResult<()> {
    if ran.code == Some(0) {
        return Ok(());
    }
    let message = ran.stderr.trim();
    Err(failed(if message.is_empty() { fallback } else { message }))
}

pub(crate) fn deliver(node: Option<&Map<String, Value>>, body: &str, root: &Path, org_file: &Path, deadline: Option<Instant>) -> NorthResult<()> {
    let Some(mut node) = node.filter(|node| text(node, "state") == "active").cloned() else {
        return Err(failed("recipient is not an active registered node"));
    };
    let proxy = text(&node, "id") == "proxy";
    if proxy {
        inbox(&node, org_file, body, deadline)?;
        if text(&node, "provider") != "codex" {
            return Ok(());
        }
        if health(&node) != "active" {
            return Err(failed("registered Codex proxy is unavailable; restore its existing control session"));
        }
    }
    if text(&node, "provider") == "claude" {
        if !truthy(node.get("status_file")) {
            return Err(failed("registered Claude session has no inbox"));
        }
        return inbox(&node, org_file, body, deadline);
    }
    if !proxy && health(&node) != "active" {
        let id = text(&node, "id").to_string();
        let argv = [root.join("bin/agents").display().to_string(), "lead".into(), "restart".into(), id.clone()];
        check(run(&argv, &[], Duration::from_secs(80), deadline)?, "recipient restart failed")?;
        node = load_registry(org_file)?.remove(&id).ok_or_else(|| failed(format!("{id} is not registered after restart")))?;
    }
    if text(&node, "provider") == "codex" {
        let argv = [root.join("bin/codex-lead").display().to_string(), "send".into(), body.into()];
        check(run(&argv, &[("CODEX_LEAD_THREAD", text(&node, "session"))], Duration::from_secs(30), deadline)?, "Codex delivery failed")?;
    }
    Ok(())
}

pub(crate) fn on_main(row: &EventRow, home: &Path) -> NorthResult<bool> {
    let (Some(lane), Some(sha)) = (row.lane.as_deref().filter(|lane| !lane.is_empty()), row.sha.as_deref().filter(|sha| !sha.is_empty())) else {
        return Ok(false);
    };
    let mut location = lane.to_string();
    if let Some(repo) = row.repo.as_deref().filter(|repo| !repo.is_empty() && !Path::new(lane).is_dir()) {
        location = home.join("code").join(repo).join("main").display().to_string();
    }
    let git = |arguments: &[&str], seconds| {
        let argv: Vec<String> = ["git", "-C", &location].iter().chain(arguments).map(|part| part.to_string()).collect();
        run(&argv, &[], Duration::from_secs(seconds), None)
    };
    if git(&["fetch", "-q", "origin", "main"], 10)?.code != Some(0) {
        return Ok(false);
    }
    Ok(git(&["merge-base", "--is-ancestor", sha, "origin/main"], 20)?.code == Some(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn row() -> EventRow {
        serde_json::from_str(include_str!("fixtures/events/row.json")).unwrap()
    }

    fn org(directory: &Path, nodes: Value) -> (PathBuf, Registry) {
        let path = directory.join("org.json");
        fs::write(&path, json!({"version": 1, "nodes": nodes}).to_string()).unwrap();
        let registry = load_registry(&path).unwrap();
        (path, registry)
    }

    fn node(directory: &Path, id: &str, extra: Value) -> Value {
        let mut fields = json!({"id": id, "role": "lead", "domain": "north", "parent": "proxy", "provider": "claude",
            "session": format!("{id}-session"), "status_file": directory.join(format!("{id}-status.md")).display().to_string(),
            "depth": 1, "budget": 1, "state": "active"});
        fields.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        fields
    }

    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn event_delivery_describe_matches_the_python_oracle() {
        let mut row = row();
        assert_eq!(describe(&row), include_str!("fixtures/events/describe.txt"));
        row.item = None;
        row.lane = None;
        row.sha = None;
        assert_eq!(describe(&row), include_str!("fixtures/events/describe-bare.txt"));
    }

    #[test]
    fn event_delivery_inbox_line_is_describe_and_is_not_appended_twice() {
        let directory = tempfile::tempdir().unwrap();
        let (path, registry) = org(directory.path(), json!([node(directory.path(), "north-lead", json!({})), node(directory.path(), "north-daemon", json!({}))]));
        fs::write(directory.path().join("north-lead-inbox.md"), "earlier line").unwrap();
        let row = row();
        mirror(&row, &registry, &path, None).unwrap();
        mirror(&row, &registry, &path, None).unwrap();
        deliver(registry.get("north-lead"), &describe(&row), directory.path(), &path, None).unwrap();
        let expected = include_str!("fixtures/events/describe.txt");
        assert_eq!(fs::read_to_string(directory.path().join("north-lead-inbox.md")).unwrap(), format!("earlier line\n{expected}\n"));
        assert_eq!(fs::read_to_string(directory.path().join("north-daemon-status.md")).unwrap(), format!("{expected}\n"));
    }

    #[test]
    fn event_delivery_north_hosted_recipient_gets_no_inbox_line() {
        let directory = tempfile::tempdir().unwrap();
        let (path, registry) = org(directory.path(), json!([node(directory.path(), "north-lead", json!({"host": "north"})), node(directory.path(), "north-daemon", json!({}))]));
        let row = row();
        mirror(&row, &registry, &path, None).unwrap();
        deliver(registry.get("north-lead"), &describe(&row), directory.path(), &path, None).unwrap();
        assert!(!directory.path().join("north-lead-inbox.md").exists());
        assert!(directory.path().join("north-daemon-status.md").exists());
    }

    #[test]
    fn event_delivery_refusals_detect_finished_removed_and_dead_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let (_, registry) = org(directory.path(), json!([
            node(directory.path(), "done", json!({})),
            node(directory.path(), "gone", json!({"pid": dead_pid()})),
            node(directory.path(), "settled", json!({"state": "finished"})),
            node(directory.path(), "live", json!({"pid": std::process::id()})),
        ]));
        fs::write(directory.path().join("done-status.md"), "working\nFINISHED landed\n\n").unwrap();
        let status = directory.path().join("done-status.md").display().to_string();
        assert_eq!(refusal(&registry, "done"), Some(format!("FINISHED in {status}")));
        assert_eq!(refusal(&registry, "missing"), Some("removed from the org".into()));
        assert_eq!(refusal(&registry, "gone"), Some("a dead session".into()));
        assert_eq!(refusal(&registry, "settled"), Some("finished in the org".into()));
        assert_eq!(refusal(&registry, "live"), None);
        assert_eq!(refusal(&registry, "proxy"), None);
    }

    #[test]
    fn event_delivery_codex_node_runs_codex_lead_send_with_its_thread() {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let record = directory.path().join("argv.txt");
        let stub = bin.join("codex-lead");
        fs::write(&stub, format!("#!/bin/sh\nprintf '%s\\n' \"$CODEX_LEAD_THREAD\" \"$@\" > '{}'\n", record.display())).unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let (path, registry) = org(directory.path(), json!([node(directory.path(), "codex-lead", json!({"provider": "codex", "session": "thread-77"}))]));
        deliver(registry.get("codex-lead"), "EVENT body", directory.path(), &path, None).unwrap();
        assert_eq!(fs::read_to_string(&record).unwrap(), "thread-77\nsend\nEVENT body\n");
        assert!(!directory.path().join("codex-lead-inbox.md").exists());
    }

    #[test]
    fn event_delivery_progressed_by_status_mtime_or_a_later_child() {
        let directory = tempfile::tempdir().unwrap();
        let now = Timestamp::now().unix_seconds();
        let mut row = row();
        row.time = Timestamp::from_unix_seconds(now - 60).iso();
        let (_, registry) = org(directory.path(), json!([node(directory.path(), "north-lead", json!({}))]));
        assert!(!progressed(&row, &registry).unwrap());
        fs::write(directory.path().join("north-lead-status.md"), "moving\n").unwrap();
        assert!(progressed(&row, &registry).unwrap());
        row.time = Timestamp::from_unix_seconds(now + 60).iso();
        assert!(!progressed(&row, &registry).unwrap());
        let started = format!("{}+0000", Timestamp::from_unix_seconds(now + 120).iso().trim_end_matches('Z'));
        let (_, registry) = org(directory.path(), json!([node(directory.path(), "north-lead", json!({})),
            node(directory.path(), "child", json!({"parent": "north-lead", "started": started}))]));
        assert!(progressed(&row, &registry).unwrap());
    }

    #[test]
    fn event_delivery_on_main_checks_the_sha_against_origin_main() {
        let directory = tempfile::tempdir().unwrap();
        let git = |arguments: &[&str]| {
            let output = crate::platform::git::run(directory.path(), arguments).unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&["clone", "-q", "origin.git", "lane"]);
        git(&["-C", "lane", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "one"]);
        git(&["-C", "lane", "push", "-q", "origin", "HEAD:main"]);
        let landed = git(&["-C", "lane", "rev-parse", "HEAD"]);
        git(&["-C", "lane", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "two"]);
        let local = git(&["-C", "lane", "rev-parse", "HEAD"]);
        let mut row = row();
        row.lane = Some(directory.path().join("lane").display().to_string());
        row.sha = Some(landed);
        assert!(on_main(&row, directory.path()).unwrap());
        row.sha = Some(local);
        assert!(!on_main(&row, directory.path()).unwrap());
    }
}
