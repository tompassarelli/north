//! The frames a session's host wrote: where its stream is indexed, and the transcript they add up to.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{SessionRecord, Transcript};
use crate::fleet::runtime::Runtime;
use crate::fleet::transcript::{current_session_stream_index, ensure_session_streams, read_window, session_stream_path};

use super::status;
use super::Fleet;

impl Fleet {
    pub(super) fn recent(&self, runtime: &dyn Runtime, record: &SessionRecord) -> NorthResult<Transcript> {
        let (path, _) = self.stream_index(record)?;
        let mut transcript = seeded(record);
        for frame in read_frames(&path)? {
            runtime.observe(&frame, &mut transcript);
        }
        if let Some(status) = status::latest_turn_status(&path, 0, record.conversation.as_deref(), &|frame| runtime.signals(frame))? {
            transcript.status = status;
        }
        Ok(transcript)
    }

    pub(super) fn recent_frames(&self, record: &SessionRecord) -> NorthResult<Vec<serde_json::Value>> {
        let (path, _) = self.stream_index(record)?;
        read_frames(&path)
    }

    pub(super) fn stream_index(&self, record: &SessionRecord) -> NorthResult<(PathBuf, u64)> {
        self.stream_index_among(record, || {
            if record.host_dir == self.root.join(&record.name) {
                return Ok(vec![record.clone()]);
            }
            Ok(self.sessions()?.into_iter().filter(|session| session.host_dir == record.host_dir).collect())
        })
    }

    pub(super) fn stream_index_among(&self, record: &SessionRecord, siblings: impl FnOnce() -> NorthResult<Vec<SessionRecord>>) -> NorthResult<(PathBuf, u64)> {
        if record.host_dir.as_os_str().is_empty() {
            return Err(NorthError::State(format!("{} has no host stream", record.name)));
        }
        if let Some(indexed_to) = current_session_stream_index(&record.host_dir, &record.name) {
            return Ok((session_stream_path(&record.host_dir, &record.name), indexed_to));
        }
        let records = siblings()?;
        let runtime = self.runtime(record)?;
        let indexed_to = ensure_session_streams(&record.host_dir, &records, |frame| runtime.frame_conversation(frame).map(str::to_owned))?;
        Ok((session_stream_path(&record.host_dir, &record.name), indexed_to))
    }
}

pub(super) fn seeded(record: &SessionRecord) -> Transcript {
    let mut transcript = Transcript::new();
    transcript.name = record.name.clone();
    transcript.conversation = record.conversation.clone();
    transcript
}

fn read_frames(path: &Path) -> NorthResult<Vec<serde_json::Value>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let bytes = read_window(&mut file, 0, u64::MAX)?;
    let complete = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
    Ok(bytes[..complete]
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect())
}
