use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::land::telemetry::epoch;
use crate::land::trace::check_log::{self, Mark};
use crate::landing::check_findings;
use super::LOG_KEEP;

const ERROR_WORDS: &[&str] = &["error", "not found", "fail", "panicked", "denied", "no such"];
const FIRST_ERROR_BOUND: usize = 300;

pub(super) fn log_findings(text: &str) -> std::collections::BTreeSet<String> {
    let found = check_findings::findings(text);
    if found.is_empty() { first_error(text).into_iter().collect() } else { found }
}

pub(super) fn first_error(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let line = lines.iter().find(|line| {
        let lower = line.to_lowercase();
        ERROR_WORDS.iter().any(|word| lower.contains(word))
    }).or_else(|| lines.last())?;
    let mut cut = line.len().min(FIRST_ERROR_BOUND);
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    Some(line[..cut].to_owned())
}

fn log_directory() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    let directory = state.join("safe-push");
    fs::create_dir_all(&directory).ok()?;
    fs::canonicalize(directory).ok()
}

fn prune_logs(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("land-check-") && name.ends_with(".log")) {
            continue;
        }
        let old = entry.metadata().and_then(|meta| meta.modified()).ok().and_then(|time| SystemTime::now().duration_since(time).ok()).is_some_and(|age| age > LOG_KEEP);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub(super) fn new_log() -> Option<PathBuf> {
    let directory = log_directory()?;
    prune_logs(&directory);
    let file = tempfile::Builder::new().prefix("land-check-").suffix(".log").rand_bytes(10).tempfile_in(&directory).ok()?;
    file.keep().ok().map(|(_, path)| path)
}

pub(super) enum Sink {
    Out,
    Err,
}

pub(super) async fn tee(mut source: impl AsyncRead + Unpin, log: PathBuf, sink: Sink) -> Vec<Mark> {
    let mut marks = Vec::new();
    let Ok(mut file) = OpenOptions::new().append(true).open(&log) else { return marks };
    let mut buffer = [0u8; 8192];
    let mut pending = Vec::new();
    let (mut opened, mut previous) = (true, None::<Mark>);
    while let Ok(count) = source.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        let _ = file.write_all(&buffer[..count]);
        pending.extend_from_slice(&buffer[..count]);
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line = String::from_utf8_lossy(&pending[..end]).into_owned();
            pending.drain(..=end);
            let at = epoch();
            if check_log::is_marker(&line) {
                if check_log::is_released(&line) {
                    marks.extend(previous.take());
                }
                marks.push(Mark { at, line });
                opened = true;
            } else if opened {
                marks.push(Mark { at, line });
                opened = false;
            } else {
                previous = Some(Mark { at, line });
            }
        }
        match sink {
            Sink::Out => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&buffer[..count]);
                let _ = out.flush();
            }
            Sink::Err => {
                let mut out = std::io::stderr().lock();
                let _ = out.write_all(&buffer[..count]);
                let _ = out.flush();
            }
        }
    }
    marks.extend(previous);
    marks
}
