use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};

pub(super) struct Placed {
    pub id: u64,
    pub pid: u32,
    pub workspace: u64,
    pub column: u64,
}

fn niri(arguments: &[&str]) -> NorthResult<String> {
    let output = Command::new("niri").arg("msg").args(arguments).output()?;
    if !output.status.success() {
        return Err(NorthError::State(format!("niri msg {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn windows() -> NorthResult<Vec<Value>> {
    serde_json::from_str::<Vec<Value>>(&niri(&["-j", "windows"])?).map_err(NorthError::Json)
}

fn placed(window: &Value) -> Option<Placed> {
    Some(Placed {
        id: window["id"].as_u64()?,
        pid: u32::try_from(window["pid"].as_u64()?).ok()?,
        workspace: window["workspace_id"].as_u64()?,
        column: window["layout"]["pos_in_scrolling_layout"][0].as_u64().unwrap_or(1),
    })
}

pub(super) fn window_of_terminal(terminal: u32) -> NorthResult<Placed> {
    windows()?.iter().filter_map(placed).find(|window| window.pid == terminal).ok_or_else(|| NorthError::State(format!("no desktop window belongs to terminal process {terminal}")))
}

pub(super) fn ids() -> NorthResult<Vec<u64>> {
    Ok(windows()?.iter().filter_map(|window| window["id"].as_u64()).collect())
}

pub(super) fn focused() -> NorthResult<Option<u64>> {
    Ok(windows()?.iter().find(|window| window["is_focused"] == true).and_then(|window| window["id"].as_u64()))
}

pub(super) fn open(command: &[String]) -> NorthResult<Placed> {
    let before = ids()?;
    let mut arguments = vec!["action", "spawn", "--"];
    arguments.extend(command.iter().map(String::as_str));
    niri(&arguments)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(window) = windows()?.iter().filter(|window| window["app_id"] == "com.mitchellh.ghostty" && window["id"].as_u64().is_some_and(|id| !before.contains(&id))).find_map(placed) {
            return Ok(window);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(NorthError::State("the replacement window did not open within 20 seconds".into()))
}

pub(super) fn focus(id: u64) -> NorthResult<()> {
    niri(&["action", "focus-window", "--id", &id.to_string()]).map(drop)
}

pub(super) fn settle(new: &Placed, old: &Placed, restore: Option<u64>) -> NorthResult<()> {
    let id = new.id.to_string();
    if new.workspace != old.workspace {
        let index = serde_json::from_str::<Vec<Value>>(&niri(&["-j", "workspaces"])?)?.iter().find(|space| space["id"].as_u64() == Some(old.workspace)).and_then(|space| space["idx"].as_u64());
        if let Some(index) = index {
            niri(&["action", "move-window-to-workspace", "--window-id", &id, &index.to_string()])?;
        }
    }
    focus(new.id)?;
    niri(&["action", "move-column-to-index", &old.column.to_string()])?;
    match restore.filter(|focused| *focused != old.id) {
        Some(other) => focus(other),
        None => Ok(()),
    }
}

pub(super) fn terminate(pid: u32) {
    let _ = Command::new("kill").arg(pid.to_string()).status();
}

pub(super) fn hang_up(pid: u32) {
    let _ = Command::new("kill").args(["-HUP", &pid.to_string()]).status();
}
