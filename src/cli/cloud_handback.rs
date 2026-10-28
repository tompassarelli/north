use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

const BEGIN: &str = "BEGIN-HANDBACK part ";
const END: &str = "END-HANDBACK part ";

struct Part {
    index: usize,
    total: usize,
    body: String,
}

fn tool_outputs(events: &[Value]) -> Vec<String> {
    let blocks = events
        .iter()
        .filter_map(|event| event.pointer("/message/content")?.as_array())
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"));
    blocks
        .map(|block| match block.get("content") {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        })
        .collect()
}

fn part_of(output: &str) -> Option<Part> {
    let mut lines = output.lines();
    let header = lines.by_ref().find_map(|line| line.strip_prefix(BEGIN))?;
    let (index, total) = header.trim().split_once('/')?;
    let body = lines
        .take_while(|line| !line.starts_with(END))
        .map(str::trim)
        .collect::<String>();
    Some(Part {
        index: index.parse().ok()?,
        total: total.parse().ok()?,
        body,
    })
}

fn latest_handback(events: &[Value]) -> Result<String, String> {
    let parts = tool_outputs(events)
        .iter()
        .filter_map(|output| part_of(output))
        .collect::<Vec<_>>();
    let total = parts
        .last()
        .map(|part| part.total)
        .ok_or("no BEGIN-HANDBACK part in the transcript")?;
    let mut by_index = BTreeMap::new();
    for part in parts.iter().rev().filter(|part| part.total == total) {
        match by_index.get(&part.index) {
            Some(newer) if *newer == part.body => {}
            Some(_) => break,
            None => {
                by_index.insert(part.index, part.body.as_str());
            }
        }
    }
    let missing = (1..=total)
        .filter(|index| !by_index.contains_key(index))
        .map(|index| index.to_string())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "the latest handback is corrupt: part {} of {total} is missing",
            missing.join(", ")
        ));
    }
    Ok((1..=total).map(|index| by_index[&index]).collect())
}

fn through(program: &str, args: &[&str], input: &[u8], what: &str) -> Result<Vec<u8>, String> {
    let error = |error: std::io::Error| format!("{what}: {program}: {error}");
    let mut file = tempfile::tempfile().map_err(error)?;
    file.write_all(input).map_err(error)?;
    std::io::Seek::rewind(&mut file).map_err(error)?;
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::from(file))
        .stderr(Stdio::null())
        .output()
        .map_err(error)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!("the latest handback is corrupt: {what} failed"))
    }
}

pub(super) fn collect(events: &[Value]) -> Result<Vec<u8>, String> {
    let encoded = latest_handback(events)?;
    let compressed = through("base64", &["-d"], encoded.as_bytes(), "base64 decode")?;
    let mbox = through("gzip", &["-dc"], &compressed, "gunzip")?;
    if mbox.is_empty() {
        return Err("the session committed nothing: the handback decodes to empty output".into());
    }
    if !mbox.starts_with(b"From ") {
        return Err("the latest handback is corrupt: the patch does not start with 'From '".into());
    }
    Ok(mbox)
}

pub(super) fn run(transcript: &Path, out: &Path, report: &mut dyn Write) -> Result<(), String> {
    let error = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    let text = std::fs::read_to_string(transcript).map_err(|e| error(transcript, e))?;
    let events = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", transcript.display()))?;
    let mbox = collect(&events)?;
    File::create(out)
        .and_then(|mut file| file.write_all(&mbox))
        .map_err(|e| error(out, e))?;
    let commits = mbox
        .split(|byte| *byte == b'\n')
        .filter(|line| is_mbox_separator(line))
        .count();
    let _ = writeln!(report, "{commits} commits in {}", out.display());
    Ok(())
}

fn is_mbox_separator(line: &[u8]) -> bool {
    line.strip_prefix(b"From ")
        .is_some_and(|rest| rest.len() > 40 && rest[..40].iter().all(u8::is_ascii_hexdigit))
}

#[cfg(test)]
#[path = "cloud_handback_tests.rs"]
mod tests;
