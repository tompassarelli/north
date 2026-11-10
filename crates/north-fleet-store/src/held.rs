//! Messages the fleet holds back from waking their recipient: a worker's interim turn, an alarm's
//! cleared or recovered notice and a repeat of an alarm still open. They change nothing the recipient
//! must act on, so they wait in one log that `north fleet held` and `north work --since` print and the proxy reads hourly.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "held.jsonl";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Held {
    pub at_ms: u64,
    pub to: String,
    pub from: String,
    pub text: String,
}

pub fn hold(root: &Path, to: &str, from: &str, text: &str, at_ms: u64) {
    let row = Held { at_ms, to: to.to_owned(), from: from.to_owned(), text: text.to_owned() };
    let Ok(mut line) = serde_json::to_string(&row) else { return };
    line.push('\n');
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(root.join(FILE)) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn since(root: &Path, since_ms: u64) -> Vec<Held> {
    fs::read_to_string(root.join(FILE)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str::<Held>(line).ok()).filter(|held| held.at_ms >= since_ms).collect()
}
