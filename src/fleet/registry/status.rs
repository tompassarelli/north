use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::error::NorthResult;
use crate::fleet::model::Status;
use crate::fleet::turns::TurnSignal;
use crate::fleet::transcript::{WINDOW_SIZE, read_window};

pub(super) fn latest_turn_status(
    path: &Path,
    offset: u64,
    signals: &dyn Fn(&Value) -> Vec<TurnSignal>,
) -> NorthResult<Option<Status>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut cursor = file.metadata()?.len();
    let offset = offset.min(cursor);
    let mut skip_partial_tail = cursor > offset;
    if skip_partial_tail {
        file.seek(SeekFrom::Start(cursor - 1))?;
        let mut last_byte = [0];
        file.read_exact(&mut last_byte)?;
        skip_partial_tail = last_byte[0] != b'\n';
    }
    let mut remainder = Vec::new();
    while cursor > offset {
        let start = cursor.saturating_sub(WINDOW_SIZE).max(offset);
        let mut bytes = read_window(&mut file, start, cursor - start)?;
        bytes.extend(remainder);
        let mut end = bytes.len();
        if skip_partial_tail {
            match bytes.iter().rposition(|byte| *byte == b'\n') {
                Some(newline) => {
                    end = newline + 1;
                    skip_partial_tail = false;
                }
                None => {
                    remainder = bytes;
                    cursor = start;
                    continue;
                }
            }
        }
        while let Some(newline) = bytes[..end].iter().rposition(|byte| *byte == b'\n') {
            if let Some(status) = frame_status(&bytes[newline + 1..end], signals) {
                return Ok(Some(status));
            }
            end = newline;
        }
        if start == offset {
            return Ok(frame_status(&bytes[..end], signals));
        }
        remainder = bytes[..end].to_vec();
        cursor = start;
    }
    Ok(None)
}

fn frame_status(line: &[u8], signals: &dyn Fn(&Value) -> Vec<TurnSignal>) -> Option<Status> {
    let frame = serde_json::from_slice::<Value>(line).ok()?;
    signals(&frame).into_iter().rev().find_map(|signal| match signal {
        TurnSignal::Started { .. } | TurnSignal::Prompt { .. } => Some(Status::Working),
        TurnSignal::Ended { .. } => Some(Status::Idle),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::fleet::turns::{TurnExit, TurnTotals};

    use super::*;

    fn signals(frame: &Value) -> Vec<TurnSignal> {
        let conversation = "s".to_owned();
        match frame["type"].as_str() {
            Some("user") => vec![TurnSignal::Started { conversation }],
            Some("result") => vec![TurnSignal::Ended {
                conversation,
                exit: TurnExit::Completed,
                totals: TurnTotals::default(),
            }],
            _ => Vec::new(),
        }
    }

    #[test]
    fn status_uses_the_latest_turn_boundary_past_the_recent_window() {
        let root = tempfile::tempdir().unwrap();
        let stream = root.path().join("stream.jsonl");
        let user = json!({"type": "user"}).to_string();
        let result = json!({"type": "result"}).to_string();
        let padding = format!("{{\"type\":\"system\",\"padding\":\"{}\"}}", "x".repeat(65_536));

        for (index, (frames, expected)) in [
            (vec![user.clone()], Status::Working),
            (vec![user, result.clone()], Status::Idle),
        ]
        .into_iter()
        .enumerate()
        {
            let mut data = format!("{}\n", frames.join("\n"));
            for _ in 0..17 {
                data.push_str(&padding);
                data.push('\n');
            }
            if index == 0 {
                data.push_str(&result);
            }
            std::fs::write(&stream, data).unwrap();
            assert!(std::fs::metadata(&stream).unwrap().len() > (1 << 20));
            assert_eq!(latest_turn_status(&stream, 0, &signals).unwrap(), Some(expected), "scenario {index}");
        }
    }
}
