use std::path::PathBuf;
use std::sync::Arc;

use crate::error::NorthResult;

use super::super::model::{SessionRecord, Transcript};
use super::super::records::{read_record, write_record};
use super::super::runtime::{Runtime, now_ms};
use super::super::transcript::{StreamTail, stream_path};
use super::{seeded, settled_status};

pub struct Follower {
    runtime: Arc<dyn Runtime>,
    record: SessionRecord,
    session_dir: PathBuf,
    tail: StreamTail,
    transcript: Transcript,
}

impl Follower {
    pub(super) fn open(runtime: Arc<dyn Runtime>, record: SessionRecord, session_dir: PathBuf) -> NorthResult<Self> {
        let tail = StreamTail::open(stream_path(&record.host_dir), record.stream_offset);
        let transcript = seeded(&record);
        let mut follower = Self { runtime, record, session_dir, tail, transcript };
        follower.poll()?;
        Ok(follower)
    }

    pub fn poll(&mut self) -> NorthResult<bool> {
        let before = (self.transcript.entries.len(), self.transcript.status);
        self.refresh_record();
        let frames = self.tail.frames()?;
        if !frames.is_empty() {
            self.transcript.last_activity_ms = Some(now_ms());
        }
        for frame in &frames {
            if let Some(quota) = self.runtime.quota(frame) {
                self.transcript.quota = Some(quota);
            }
            project(self.runtime.as_ref(), frame, &mut self.transcript);
        }
        self.persist_conversation()?;
        self.transcript.status = settled_status(&self.record, self.transcript.status);
        Ok(!frames.is_empty() || before != (self.transcript.entries.len(), self.transcript.status))
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn record(&self) -> &SessionRecord {
        &self.record
    }

    fn refresh_record(&mut self) {
        if let Ok(record) = read_record(&self.session_dir) {
            self.record = record;
        }
    }

    fn persist_conversation(&mut self) -> NorthResult<()> {
        let observed = self.transcript.conversation.clone();
        if observed.is_none() || observed == self.record.conversation {
            return Ok(());
        }
        self.record.conversation = observed;
        write_record(&self.session_dir, &self.record)
    }
}

fn project(runtime: &dyn Runtime, frame: &serde_json::Value, transcript: &mut Transcript) {
    runtime.observe(frame, transcript);
    for signal in runtime.signals(frame) {
        if transcript.conversation.as_deref() != Some(signal.conversation()) {
            continue;
        }
        if let Some(turn) = transcript.tracker.observe(signal, now_ms()) {
            transcript.turns.push(turn);
        }
    }
}

