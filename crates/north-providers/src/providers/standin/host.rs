use std::hash::{Hash, Hasher};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use serde_json::{Value, json};

const PHASE_FRAME: &str = "north_turn";
const USAGE: &str = "usage: north standin-host --session ID";
const DIES: &str = "dies once mid-lane";
const DIED: i32 = 137;

pub(super) fn session(frame: &Value) -> Option<&str> {
    frame["sessionID"].as_str().filter(|session| !session.is_empty())
}

pub(super) fn phase(frame: &Value) -> Option<&str> {
    (frame["type"] == PHASE_FRAME).then(|| frame["phase"].as_str()).flatten()
}

/// Serves a stand-in session on stdin and stdout: each user frame is one turn that lands one lane.
pub fn run(arguments: &[String]) -> u8 {
    let [flag, session] = arguments else {
        eprintln!("{USAGE}");
        return 64;
    };
    if flag != "--session" {
        eprintln!("{USAGE}");
        return 64;
    }
    let checkout = std::env::current_dir().unwrap_or_default();
    let mut out = std::io::stdout().lock();
    let mut emit = |frame: Value| {
        let _ = writeln!(out, "{frame}");
        let _ = out.flush();
    };
    emit(json!({"type": PHASE_FRAME, "phase": "ready", "sessionID": session}));
    let mut first = true;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(frame) = serde_json::from_str::<Value>(&line) else { continue };
        if frame["type"] != "user" {
            continue;
        }
        let text = frame["text"].as_str().unwrap_or_default();
        let started = Instant::now();
        emit(json!({"type": PHASE_FRAME, "phase": "start", "sessionID": session, "uuid": frame["uuid"], "text": text, "first": first}));
        first = false;
        let (report, exit) = match turn(&checkout, text) {
            Ok(ticket) => (format!("Done: {ticket}"), "completed"),
            Err(why) => (format!("Not done: {why}\n\nStop: reasoning"), "failed"),
        };
        emit(json!({"type": "text", "sessionID": session, "text": report}));
        emit(json!({"type": PHASE_FRAME, "phase": "end", "sessionID": session, "exit": exit, "wall_ms": started.elapsed().as_millis() as u64}));
    }
    0
}

fn declared<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix(':')).map(str::trim).filter(|value| !value.is_empty())
}

fn turn(checkout: &Path, brief: &str) -> Result<String, String> {
    let item = declared(brief, "Item").ok_or("the brief names no Item: line")?;
    let box_text = declared(brief, "Box").ok_or("the brief names no Box: line")?;
    let (repo, number) = item.rsplit_once('#').ok_or_else(|| format!("Item: {item} is not REPO#N"))?;
    let repo = repo.rsplit('/').next().unwrap_or(repo);
    let plain = !repo.is_empty() && repo != "." && repo != ".." && repo.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
    let number: u64 = number.parse().map_err(|_| format!("Item: {item} has no issue number after #"))?;
    let project = checkout.parent().ok_or("the session's checkout has no project directory")?;
    if !plain || project.file_name().is_none_or(|own| own.to_string_lossy() != repo) {
        return Err(format!("Item: {item} is not this checkout's repository {}", project.display()));
    }
    let mut digest = std::collections::hash_map::DefaultHasher::new();
    (item, box_text).hash(&mut digest);
    let name = format!("si{number}-{:08x}", digest.finish() as u32);
    let lane = project.join("worktrees").join(&name);
    let fresh = !lane.exists();
    if fresh {
        sandboxed(project, &format!("north lane new --issue {repo}#{number} {}", quoted(&lane.display().to_string())))?;
    }
    if fresh && box_text.contains(DIES) {
        std::process::exit(DIED);
    }
    let file = format!("lanes/standin/{name}");
    let message = format!("Stand-in lane for {repo}#{number}\n\nItem: {item}\nBox: {box_text}");
    let printed = sandboxed(&lane, &format!("mkdir -p lanes/standin && printf '%s\\n' {name} > {file} && north hand-in --message {} -- {file}", quoted(&message)))?;
    Ok(printed.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("handed in").trim().to_owned())
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn sandboxed(directory: &Path, command: &str) -> Result<String, String> {
    let output = Command::new(north_base::platform::north_binary::north_binary()).args(["--worker-sandbox", command]).current_dir(directory).output().map_err(|why| format!("north --worker-sandbox: {why}"))?;
    let printed = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if output.status.success() { Ok(printed) } else { Err(format!("`{command}` exited {}: {}", output.status, printed.trim())) }
}
