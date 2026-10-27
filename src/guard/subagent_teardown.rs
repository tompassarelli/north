pub mod transcript;

use crate::hook::{HookInput, Verdict};
use crate::pyjson::Value;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use transcript::{Kind, Task};

const MONITOR_CEILING_MS: f64 = 3_600_000.0;

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook") {
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
    Some(format!(
        "You still have background tasks running: {}. Stop each with TaskStop, then finish your report.",
        names.join(", ")
    ))
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
