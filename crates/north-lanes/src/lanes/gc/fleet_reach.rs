use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use toml_edit::DocumentMut;

use super::exec::command;
use super::liveness::alive_status;
use north_base::platform::{host, orchestration, process};

const BUILD_HOST_RUN: Duration = Duration::from_secs(25 * 60);
const REMOTE_NORTH: &str = "~/.local/state/north/current/north";

fn orchestration_file(home: &std::path::Path) -> Option<DocumentMut> {
    std::fs::read_to_string(orchestration::path_in(home, None, None)).ok()?.parse().ok()
}

pub(super) fn on_registry_host(home: &std::path::Path) -> bool {
    orchestration::owners_machine(&orchestration::path_in(home, None, None)).is_some_and(|name| name == host::name())
}

pub(super) fn supplied(wanted: bool, known: Option<Vec<Value>>) -> Result<Option<Vec<Value>>, String> {
    if wanted && known.is_none() { sessions_from_stdin().map(Some).map_err(|error| format!("sessions on stdin: {error}")) } else { Ok(known) }
}

pub(super) fn refuses(sessions_known: bool, home: &std::path::Path) -> bool {
    let refused = !sessions_known && !on_registry_host(home);
    if refused {
        println!("lane gc: this host does not hold the fleet registry and the laptop passed no session list; kept everything");
    }
    refused
}

fn sessions_from_stdin() -> Result<Vec<Value>, String> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|error| error.to_string())?;
    match serde_json::from_str(&text).map_err(|error| error.to_string())? {
        Value::Array(sessions) => Ok(sessions),
        _ => Err("session list on stdin is not an array".into()),
    }
}

pub(super) fn live_on(sessions: &[Value], machine: &str) -> Vec<Value> {
    sessions
        .iter()
        .filter(|session| session["machine"].as_str() == Some(machine) && alive_status(session))
        .map(|session| json!({"name": session["name"], "parent": session["parent"], "cwd": session["cwd"], "status": session["status"], "conversation": session["conversation"], "provider": session["provider"]}))
        .collect()
}

pub(super) fn sweep_build_hosts(home: &std::path::Path, dry_run: bool) {
    let Some(cfg) = orchestration_file(home) else { return };
    let listed = command(&["north", "fleet", "list", "--json"]);
    let Some(sessions) = listed.ok.then(|| serde_json::from_str::<Vec<Value>>(&listed.out).ok()).flatten() else {
        println!("lane gc: fleet list unreadable, so no build host was swept");
        return;
    };
    for (name, ssh) in orchestration::build_hosts(&cfg) {
        let live = live_on(&sessions, &name);
        let mut call = Command::new("ssh");
        call.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", &ssh, REMOTE_NORTH, "lane", "gc", "--sessions-stdin"]);
        if dry_run {
            call.arg("--dry-run");
        }
        call.stdout(Stdio::piped()).stderr(Stdio::piped());
        let input = serde_json::to_vec(&live).unwrap_or_default();
        match process::run_command_within(call, Some(&input), BUILD_HOST_RUN) {
            Some(output) if output.status.success() => println!("lane gc: {name}: swept with {} live sessions: {}", live.len(), String::from_utf8_lossy(&output.stdout).lines().last().unwrap_or("")),
            Some(output) => println!("lane gc: {name}: not swept: {}", String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("failed")),
            None => println!("lane gc: {name}: not swept: timed out"),
        }
    }
}
