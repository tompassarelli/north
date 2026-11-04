use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::queue;

pub const RETRY_SECONDS: u64 = 300;
pub const FILE: &str = "base-broken";
const ALERT_TITLE: &str = "Lander paused";

#[derive(Serialize, Deserialize)]
struct Pause {
    detail: String,
    since: u64,
    retry_at: u64,
    alerted: bool,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn path(queue: &Path) -> PathBuf {
    queue.join(FILE)
}

fn load(queue: &Path) -> Option<Pause> {
    serde_json::from_str(&fs::read_to_string(path(queue)).ok()?).ok()
}

fn alert(detail: &str) -> bool {
    Command::new("supply-alert").args(["normal", detail, ALERT_TITLE]).status().is_ok_and(|status| status.success())
}

fn waiting(pause: &Pause) -> String {
    format!("{}; new candidates are paused and the base check is retried in {} s", pause.detail, pause.retry_at.saturating_sub(now()))
}

pub fn held(queue: &Path) -> Option<String> {
    let pause = load(queue)?;
    (pause.retry_at > now()).then(|| waiting(&pause))
}

pub fn raise(queue: &Path, detail: &str) -> Result<String, String> {
    let earlier = load(queue);
    let alerted = earlier.as_ref().is_some_and(|pause| pause.alerted) || alert(detail);
    if !alerted {
        eprintln!("north land: supply-alert failed for: {detail}");
    }
    let pause = Pause { detail: detail.to_owned(), since: earlier.map_or_else(now, |pause| pause.since), retry_at: now() + RETRY_SECONDS, alerted };
    let text = serde_json::to_string(&pause).map_err(|error| error.to_string())?;
    queue::write_atomic(&path(queue), &text).map_err(|error| format!("{}: {error}", path(queue).display()))?;
    Ok(waiting(&pause))
}

pub fn resume(queue: &Path) {
    if load(queue).is_some() && fs::remove_file(path(queue)).is_ok() {
        println!("north land: main's check environment passes again; candidates resume");
    }
}
