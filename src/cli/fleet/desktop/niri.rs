use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};

pub(in crate::cli::fleet) struct Placed {
    pub id: u64,
}

fn niri(arguments: &[&str]) -> NorthResult<String> {
    let mut command = Command::new("niri");
    if let Some(socket) = socket() {
        command.env("NIRI_SOCKET", socket);
    }
    let output = command.arg("msg").args(arguments).output()?;
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
    })
}

pub(in crate::cli::fleet) fn ids() -> NorthResult<Vec<u64>> {
    Ok(windows()?.iter().filter_map(|window| window["id"].as_u64()).collect())
}

pub(in crate::cli::fleet) fn open(command: &[String]) -> NorthResult<Placed> {
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
    Err(NorthError::State("the new terminal window did not open within 20 seconds".into()))
}

/// The live compositor's socket: `NIRI_SOCKET` when it names one, else the one niri keeps in the user's runtime directory, which a fleet session started outside the graphical session can still reach.
fn socket() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("NIRI_SOCKET").map(PathBuf::from).filter(|path| path.exists()) {
        return Some(named);
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    std::fs::read_dir(runtime).ok()?.flatten().map(|entry| entry.path()).find(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("niri.") && name.ends_with(".sock")))
}

/// Whether a graphical session is live on this host, or why not.
pub(in crate::cli::fleet) fn live() -> Result<(), String> {
    socket().map(drop).ok_or_else(|| "no graphical session is live on this host (no niri socket)".to_owned())
}

/// Opens `command` in a ghostty window that niri spawns, so the window is the compositor's child and outlives the north process that asked for it; returns the window's id.
pub(in crate::cli::fleet) fn open_terminal(command: &[String]) -> Result<u64, String> {
    live()?;
    let mut terminal = vec!["ghostty".to_owned(), "-e".to_owned()];
    terminal.extend_from_slice(command);
    open(&terminal).map(|placed| placed.id).map_err(|error| error.user_message())
}
