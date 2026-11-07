//! Claude Code runs the Bash hooks for a Monitor call as tool Bash without its timeout_ms, so only the transcript tells a Monitor call apart.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const TAIL_BYTES: u64 = 4 << 20;

pub fn in_transcripts<'a>(transcripts: impl IntoIterator<Item = &'a str>, id: Option<&str>, command: &str) -> bool {
    transcripts
        .into_iter()
        .filter_map(tail)
        .any(|text| names_monitor(&text, id, command))
}

fn tail(path: &str) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn names_monitor(transcript: &str, id: Option<&str>, command: &str) -> bool {
    transcript.lines().rev().filter(|line| line.contains("\"Monitor\"")).any(|line| {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        entry["message"]["content"].as_array().is_some_and(|blocks| {
            blocks.iter().any(|block| {
                block["type"] == "tool_use"
                    && block["name"] == "Monitor"
                    && id.is_none_or(|id| block["id"] == id)
                    && block["input"]["command"].as_str() == Some(command)
            })
        })
    })
}
