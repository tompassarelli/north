//! Whether Tom typed the message that opened a fleet session's current turn, read from its transcript: the evidence a
//! proxy's launch (north#734) and its gate answer on Tom's behalf (north#735) rest on.
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::fleet::gates::TypedTurn;
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;

use super::lineage;

const TAIL_BYTES: u64 = 4 << 20;

/// The proxy-role caller's current turn when Tom typed the message that opened it.
pub(in crate::cli) fn proxy_turn(fleet: &Fleet, caller: Option<&str>) -> Option<TypedTurn> {
    lineage::from_proxy(fleet, caller).then(|| typed_turn(fleet, caller?)).flatten()
}

/// The caller's current turn when Tom typed the message that opened it, read from the newest prompt in its own
/// transcript: the conversation the calling process runs in, else the one its fleet record names, on any account.
pub(super) fn typed_turn(fleet: &Fleet, caller: &str) -> Option<TypedTurn> {
    let record = fleet.sessions().ok()?.into_iter().find(|record| record.name == caller)?;
    let conversation = fleet.caller_conversation().or(record.conversation)?;
    let roster = fleet.roster();
    let homes = roster.accounts.iter().filter(|account| account.label == record.account).chain(&roster.accounts).map(|account| &account.home);
    let transcript = homes.filter_map(|home| fs::read_dir(home.join("projects")).ok()).flat_map(|entries| entries.flatten()).map(|entry| entry.path().join(format!("{conversation}.jsonl"))).find(|path| path.is_file())?;
    typed_at(&transcript).map(|at_ms| TypedTurn { conversation, at_ms })
}

/// When Tom typed the newest prompt in a transcript, if he did.
pub(in crate::cli) fn typed_at(transcript: &Path) -> Option<u64> {
    newest_prompt(transcript).filter(Prompt::typed).map(|prompt| prompt.at_ms)
}

struct Prompt {
    text: String,
    at_ms: u64,
    origin: Option<String>,
}

impl Prompt {
    fn typed(&self) -> bool {
        self.origin.as_deref().is_none_or(|origin| origin == "human") && typed(&self.text)
    }
}

/// The newest prompt in a transcript: a user row, or a message queued while the session worked, that is no tool
/// result, harness note or background task's notice.
fn newest_prompt(transcript: &Path) -> Option<Prompt> {
    let mut file = File::open(transcript).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    String::from_utf8_lossy(&tail).lines().rev().filter_map(|line| serde_json::from_str::<Value>(line).ok()).find_map(|row| prompt(&row))
}

fn prompt(row: &Value) -> Option<Prompt> {
    let flagged = |key: &str| row[key].as_bool() == Some(true);
    if flagged("isMeta") || flagged("isCompactSummary") {
        return None;
    }
    let queued = &row["attachment"];
    let (content, origin) = match row["type"].as_str()? {
        "user" => (&row["message"]["content"], &row["origin"]),
        "attachment" if queued["type"] == "queued_command" => (&queued["prompt"], if row["origin"].is_null() { &queued["origin"] } else { &row["origin"] }),
        _ => return None,
    };
    let origin = origin["kind"].as_str().map(str::to_owned);
    if origin.as_deref() == Some("task-notification") {
        return None;
    }
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().find(|block| block["type"] == "text")?["text"].as_str()?.to_owned(),
        _ => return None,
    };
    let at = Timestamp::parse_rfc3339(row["timestamp"].as_str()?)?;
    Some(Prompt { text, at_ms: (at.unix_seconds_f64() * 1000.0) as u64, origin })
}

/// Tom's words carry no sender: a session or service message starts `[from NAME]`, a gate answer `[gate ID]` and a
/// harness notice a tag.
pub(super) fn typed(text: &str) -> bool {
    let text = text.trim_start();
    !(text.starts_with("[from ") || text.starts_with("[gate ") || text.starts_with('<'))
}
