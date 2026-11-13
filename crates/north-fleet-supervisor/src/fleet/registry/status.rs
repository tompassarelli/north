use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::error::NorthResult;
use crate::fleet::gates::{self, Gate};
use crate::fleet::model::{SessionPhase, SessionRecord, Status};
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

    pub fn status(&self, record: &SessionRecord) -> Status {
        if record.adopted() {
            return self.window_status(record);
        }
        if record.host_dir.as_os_str().is_empty() {
            return settled(record, Status::Starting);
        }
        self.status_from_index(record).unwrap_or_else(|_| settled(record, Status::Starting))
    }

    pub fn gated_sessions(&self) -> BTreeSet<String> {
        let gates = self.ledger.gates(u64::MAX).unwrap_or_else(|error| {
            eprintln!("north fleet: the gate ledger is unreadable, so no session shows waiting: {}", error.user_message());
            Vec::new()
        });
        gates::decisions(gates).into_iter().filter(Gate::is_open).map(|gate| gate.request.session).collect()
    }

    pub fn phase(&self, record: &SessionRecord, gated: &BTreeSet<String>) -> SessionPhase {
        let status = self.status(record);
        let reply = (status == Status::Idle).then(|| self.last_report(record)).flatten();
        SessionPhase::of(status, gated.contains(&record.name), reply.as_ref().map(|(report, _)| report.as_str()))
    }

    fn status_from_index(&self, record: &SessionRecord) -> NorthResult<Status> {
        self.observed_status(record).map(|observed| settled(record, observed))
    }
}

pub(super) fn settled(record: &SessionRecord, observed: Status) -> Status {
    match (session_running(record), record.ended()) {
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

