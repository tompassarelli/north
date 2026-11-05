use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};

pub(super) struct Window {
    pub client: u32,
    pub profile: PathBuf,
    pub account: String,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    pub session: Session,
}

pub(super) struct Session {
    pub id: String,
    pub transcript: PathBuf,
    pub job: Option<String>,
    pub model: Option<String>,
}

pub(super) fn profiles_root() -> NorthResult<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| NorthError::Usage("no home directory".into()))?;
    Ok(home.join(".local/state/north/profiles"))
}

fn json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub(super) fn arguments_of(pid: u32) -> Vec<String> {
    fs::read(format!("/proc/{pid}/cmdline")).map(|raw| raw.split(|byte| *byte == 0).filter(|part| !part.is_empty()).map(|part| String::from_utf8_lossy(part).into_owned()).collect()).unwrap_or_default()
}

pub(super) fn environment_of(pid: u32) -> BTreeMap<String, String> {
    let raw = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    raw.split(|byte| *byte == 0).filter_map(|entry| String::from_utf8_lossy(entry).split_once('=').map(|(key, value)| (key.to_owned(), value.to_owned()))).collect()
}

pub(super) fn parent_of(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split(' ').nth(1)?.parse().ok()
}

pub(super) fn name_of(pid: u32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm")).map(|name| name.trim().to_owned()).unwrap_or_default()
}

fn on_terminal(pid: u32) -> bool {
    fs::read_link(format!("/proc/{pid}/fd/0")).is_ok_and(|target| target.to_string_lossy().starts_with("/dev/pts/"))
}

pub(super) fn find_window() -> NorthResult<Window> {
    let mut found = Vec::new();
    for profile in fs::read_dir(profiles_root()?)?.flatten().map(|entry| entry.path()) {
        let Some(account) = profile.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix("claude-")).map(str::to_owned) else { continue };
        let Ok(sessions) = fs::read_dir(profile.join("sessions")) else { continue };
        for record in sessions.flatten().filter_map(|entry| json(&entry.path())) {
            let live = record["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok()).filter(|pid| Path::new(&format!("/proc/{pid}")).exists() && on_terminal(*pid));
            if let Some(client) = live.filter(|_| record["kind"] == "interactive" && record["entrypoint"] == "cli") {
                found.push((client, profile.clone(), account.clone(), record));
            }
        }
    }
    let [(client, profile, account, record)] = <[_; 1]>::try_from(found).map_err(|found| match found.len() {
        0 => NorthError::State("no interactive claude window is running on a terminal".into()),
        count => NorthError::State(format!("{count} interactive claude windows are running; this move handles exactly one")),
    })?;
    let session = backing_session(&profile, client, &record)?;
    let cwd = record["cwd"].as_str().map(PathBuf::from).unwrap_or_default();
    let arguments = arguments_of(client).into_iter().skip(1).collect();
    Ok(Window { client, profile, account, arguments, cwd, session })
}

fn backing_session(profile: &Path, client: u32, record: &Value) -> NorthResult<Session> {
    let roster = json(&profile.join("daemon/roster.json"));
    let worker = roster.as_ref().and_then(|roster| roster["workers"].as_object()).filter(|workers| workers.len() == 1).and_then(|workers| workers.iter().next());
    let (id, job) = match worker {
        Some((short, worker)) if daemon_serves(client, roster.as_ref()) => (worker["sessionId"].as_str().map(str::to_owned), Some(short.clone())),
        _ => (record["sessionId"].as_str().map(str::to_owned), None),
    };
    let id = id.ok_or_else(|| NorthError::State("the window's session id is unreadable".into()))?;
    let state = job.as_ref().and_then(|job| json(&profile.join("jobs").join(job).join("state.json")));
    let transcript = state
        .as_ref()
        .and_then(|state| state["linkScanPath"].as_str())
        .map(PathBuf::from)
        .or_else(|| find_transcript(profile, &id))
        .ok_or_else(|| NorthError::State("the window's transcript is missing".into()))?;
    let flags = state.as_ref().and_then(|state| state["respawnFlags"].as_array()).map(|flags| flags.iter().filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default();
    let model = flags.iter().position(|flag| *flag == "--model").and_then(|at| flags.get(at + 1)).map(|model| (*model).to_owned());
    Ok(Session { id, transcript, job, model })
}

fn daemon_serves(client: u32, roster: Option<&Value>) -> bool {
    roster.and_then(|roster| roster["supervisorPid"].as_u64()).and_then(|pid| u32::try_from(pid).ok()).is_some_and(|supervisor| parent_of(supervisor) == Some(client))
}

fn find_transcript(profile: &Path, id: &str) -> Option<PathBuf> {
    fs::read_dir(profile.join("projects")).ok()?.flatten().map(|entry| entry.path().join(format!("{id}.jsonl"))).find(|path| path.exists())
}

pub(super) fn turn_ended(window: &Window) -> bool {
    match &window.session.job {
        Some(job) => json(&window.profile.join("jobs").join(job).join("state.json")).is_some_and(|state| state["state"] != "working"),
        None => json(&window.profile.join("sessions").join(format!("{}.json", window.client))).is_some_and(|record| record["status"] == "idle"),
    }
}

pub(super) fn carry(window: &Window, target: &Path) -> NorthResult<usize> {
    let id = &window.session.id;
    let project = window.session.transcript.parent().and_then(Path::file_name).ok_or_else(|| NorthError::State("the transcript has no project directory".into()))?;
    let source_project = window.session.transcript.parent().unwrap_or(&window.profile);
    let mut copied = 0;
    let plans = [
        (source_project.join(format!("{id}.jsonl")), target.join("projects").join(project).join(format!("{id}.jsonl"))),
        (source_project.join(id), target.join("projects").join(project).join(id)),
        (window.profile.join("file-history").join(id), target.join("file-history").join(id)),
        (window.profile.join("session-env").join(id), target.join("session-env").join(id)),
    ];
    for (from, to) in plans {
        if from.exists() {
            copied += copy_tree(&from, &to)?;
        }
    }
    Ok(copied)
}

fn copy_tree(from: &Path, to: &Path) -> NorthResult<usize> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        let mut count = 0;
        for entry in fs::read_dir(from)?.flatten() {
            count += copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(count);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial = to.with_extension("moving");
    fs::copy(from, &partial)?;
    fs::rename(&partial, to)?;
    Ok(1)
}

pub(super) fn activity(window: &Window) -> u64 {
    let path = match &window.session.job {
        Some(job) => window.profile.join("jobs").join(job).join("timeline.jsonl"),
        None => window.session.transcript.clone(),
    };
    fs::metadata(path).map_or(0, |meta| meta.len())
}

pub(super) fn ready_for(window: &Window, target: &Path) -> NorthResult<()> {
    let accepted = |profile: &Path| json(&profile.join(".claude.json")).map(|state| state["projects"].clone());
    let projects = accepted(target).ok_or_else(|| NorthError::State(format!("{} has no .claude.json; the account has never run claude", target.display())))?;
    let trusted = window.cwd.ancestors().any(|folder| projects[folder.to_string_lossy().as_ref()]["hasTrustDialogAccepted"] == true);
    if !trusted {
        return Err(NorthError::Refused(format!("account profile {} has not trusted {}; the replacement would stop at a trust prompt", target.display(), window.cwd.display())));
    }
    let bypass = window.arguments.iter().any(|argument| argument == "--dangerously-skip-permissions");
    let skips_prompt = json(&target.join("settings.json")).is_some_and(|settings| settings["skipDangerousModePermissionPrompt"] == true);
    if bypass && !skips_prompt {
        return Err(NorthError::Refused(format!("account profile {} would ask to confirm bypass permissions", target.display())));
    }
    Ok(())
}
