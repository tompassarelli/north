use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::error::NorthResult;
use crate::fleet::model::{SessionRecord, Status};
use crate::fleet::turns::TurnSignal;
use crate::fleet::transcript::{WINDOW_SIZE, read_window};
use crate::fleet::unaccepted::Unaccepted;

use super::{Fleet, session_running};

impl Fleet {
    /// What the stream's last turn boundary says, whether or not the host still runs.
    pub(super) fn observed_status(&self, record: &SessionRecord) -> NorthResult<Status> {
        let (path, _) = self.stream_index(record)?;
        let observed = self.runtime(record).and_then(|runtime| latest_turn_status(&path, 0, record.conversation.as_deref(), &|frame| runtime.signals(frame)))?;
        Ok(observed.unwrap_or(Status::Starting))
    }
}

pub(super) fn settled(record: &SessionRecord, observed: Status) -> Status {
    match (session_running(record), record.stopped) {
        (true, _) if Unaccepted::outstanding(&record.host_dir) => Status::Working,
        (true, _) if matches!(observed, Status::Exited | Status::Parked) => Status::Idle,
        (true, _) => observed,
        (false, true) => Status::Exited,
        (false, false) => Status::Parked,
    }
}

pub(super) fn latest_turn_status(
    path: &Path,
    offset: u64,
    conversation: Option<&str>,
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
            if let Some(status) = frame_status(&bytes[newline + 1..end], conversation, signals) {
                return Ok(Some(status));
            }
            end = newline;
        }
        if start == offset {
            return Ok(frame_status(&bytes[..end], conversation, signals));
        }
        remainder = bytes[..end].to_vec();
        cursor = start;
    }
    Ok(None)
}

fn frame_status(line: &[u8], conversation: Option<&str>, signals: &dyn Fn(&Value) -> Vec<TurnSignal>) -> Option<Status> {
    let frame = serde_json::from_slice::<Value>(line).ok()?;
    signals(&frame).into_iter().rev().filter(|signal| conversation.is_none_or(|conversation| signal.conversation() == conversation)).find_map(|signal| signal_status(&signal))
}

pub(super) fn signal_status(signal: &TurnSignal) -> Option<Status> {
    match signal {
        TurnSignal::Opened { .. } | TurnSignal::Ended { .. } => Some(Status::Idle),
        TurnSignal::Started { .. } | TurnSignal::Prompt { .. } => Some(Status::Working),
        _ => None,
    }
}

