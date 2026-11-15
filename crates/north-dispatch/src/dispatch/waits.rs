//! How long each dispatched launch's session host took to open its socket and answer its first frame, read from the launch's stderr and kept a day in `host-waits.jsonl`, so the overload page names the slowest of the last hour: the measure that settles the launcher's 60 s waits.
use super::record;
use serde::{Deserialize, Serialize};
use std::io::Write;

const FILE: &str = "host-waits.jsonl";
const KEPT_S: f64 = 24.0 * 3600.0;
const SLOWEST_S: f64 = 3600.0;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Wait {
    at: f64,
    name: String,
    socket_s: Option<f64>,
    reply_s: Option<f64>,
}

fn seconds(after: &str) -> Option<f64> {
    after.split_whitespace().next()?.parse().ok()
}

fn read_from(name: &str, stderr: &str, at: f64) -> Option<Wait> {
    for line in stderr.lines() {
        if let Some((_, rest)) = line.split_once("session host waits: socket ") {
            let reply = rest.split_once("first reply ").and_then(|(_, after)| seconds(after));
            return Some(Wait { at, name: name.to_owned(), socket_s: seconds(rest), reply_s: reply });
        }
        if let Some((_, rest)) = line.split_once("did not open its socket within ") {
            return Some(Wait { at, name: name.to_owned(), socket_s: seconds(rest), reply_s: None });
        }
        if let Some((_, rest)) = line.split_once("did not answer a frame within ") {
            return Some(Wait { at, name: name.to_owned(), socket_s: None, reply_s: seconds(rest) });
        }
    }
    None
}

fn kept(at: f64) -> Vec<Wait> {
    let text = std::fs::read_to_string(record::directory().join(FILE)).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str::<Wait>(line).ok()).filter(|wait| at - wait.at <= KEPT_S).collect()
}

/// Keeps the waits the launch `name` printed or ran out on, dropping those over a day old.
pub(super) fn record(name: &str, stderr: &str, at: f64) {
    let Some(wait) = read_from(name, stderr, at) else { return };
    let mut waits = kept(at);
    waits.push(wait);
    let text: String = waits.iter().filter_map(|wait| serde_json::to_string(wait).ok()).map(|line| line + "\n").collect();
    let path = record::directory().join(FILE);
    let written = std::fs::create_dir_all(record::directory()).and_then(|()| std::fs::File::create(&path)).and_then(|mut file| file.write_all(text.as_bytes()));
    if let Err(error) = written {
        eprintln!("north fleet dispatch: {} not written: {error}", path.display());
    }
}

/// The slowest socket open and first reply of the last hour, each with its launch.
pub(super) fn slowest(host: &str, at: f64) -> String {
    let waits: Vec<Wait> = kept(at).into_iter().filter(|wait| at - wait.at <= SLOWEST_S).collect();
    let most = |of: fn(&Wait) -> Option<f64>| waits.iter().filter_map(|wait| Some((of(wait)?, &wait.name))).max_by(|a, b| a.0.total_cmp(&b.0)).map_or_else(|| "none recorded".to_owned(), |(seconds, name)| format!("{seconds:.1} s ({name})"));
    format!("host {host} session host waits, slowest of the last hour over {} launches: socket {}, first reply {}", waits.len(), most(|wait| wait.socket_s), most(|wait| wait.reply_s))
}
