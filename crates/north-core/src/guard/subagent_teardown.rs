pub mod transcript;

use crate::hook::{HookInput, Verdict};
use crate::platform::pyjson::Value;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};
use transcript::{Kind, Task};

static LANDING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^A-Za-z0-9_.-])safe-push(?:$|[^A-Za-z0-9_.-])").unwrap());

const MONITOR_CEILING_MS: f64 = 3_600_000.0;

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    refusal(input).map_or(Verdict::Allow, Verdict::Deny)
}

fn transcript_path(input: &HookInput) -> Option<&str> {
    if transcript::has(&input.raw, b"\"stop_hook_active\":true")
        || transcript::has(&input.raw, b"\"stop_hook_active\": true")
        || !transcript::has(&input.raw, b"transcript_path\"")
        || input.data["stop_hook_active"] == Value::Bool(true)
    {
        return None;
    }
    match input.data["agent_transcript_path"].as_str() {
        Some(path) => Some(path),
        None if input.data["agent_transcript_path"].as_string().is_some() => None,
        None => input.data["transcript_path"]
            .as_str()
            .filter(|path| path.contains("/subagents/")),
    }
}

pub fn refusal(input: &HookInput) -> Option<String> {
    let fleet = std::env::var("NORTH_FLEET_SESSION").is_ok_and(|session| !session.is_empty());
    refusal_in(input, fleet)
}

pub fn refusal_in(input: &HookInput, fleet: bool) -> Option<String> {
    let path = transcript_path(input)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    let scan = transcript::scan(path, now)?;
    let mut pending: Vec<Task> = scan
        .tasks
        .into_iter()
        .filter(|task| !scan.ended.contains(&task.id))
        .filter(|task| !(task.kind == Kind::Monitor && expired(task, now)))
        .collect();
    let outputs: HashSet<&str> = pending
        .iter()
        .filter(|task| task.kind == Kind::Bash)
        .filter_map(|task| task.output.as_deref())
        .collect();
    let held = if outputs.is_empty() { HashSet::new() } else { held_outputs(&outputs) };
    pending.retain(|task| match (task.kind, task.output.as_deref()) {
        (Kind::Bash, Some(out)) => held.contains(out),
        _ => true,
    });
    pending.retain(|task| !landing(task));
    if pending.is_empty() {
        return None;
    }
    let names: Vec<String> = pending
        .iter()
        .map(|task| {
            let label = if task.description.is_empty() { task.kind.name() } else { &task.description };
            format!("{} ({label})", task.id)
        })
        .collect();
    let advice = if fleet {
        "Wait for each build, test or check to finish and act on its result; TaskStop watchers, servers and anything else whose result you no longer need, and run work that must outlive you as `job-run --notify`."
    } else {
        "Wait for each build, test or check to finish and act on its result; TaskStop watchers, servers and anything else whose result you no longer need. A background `safe-push` landing may keep running: you resume when it completes."
    };
    Some(format!("You still have background tasks running: {}. {advice}", names.join(", ")))
}

fn landing(task: &Task) -> bool {
    task.kind == Kind::Bash && LANDING.is_match(&task.command)
}

fn expired(task: &Task, now: f64) -> bool {
    let limit = if MONITOR_CEILING_MS < task.limit { MONITOR_CEILING_MS } else { task.limit };
    task.start + limit / 1000.0 < now
}

fn held_outputs(wanted: &HashSet<&str>) -> HashSet<String> {
    let mut held = HashSet::new();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return held;
    };
    for process in procs.flatten() {
        let name = process.file_name();
        if !name.as_encoded_bytes().iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(process.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path())
                && let Some(target) = target.to_str()
                && wanted.contains(target)
            {
                held.insert(target.to_owned());
            }
        }
    }
    held
}
