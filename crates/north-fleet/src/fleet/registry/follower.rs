use std::path::PathBuf;
use std::sync::Arc;

use crate::error::NorthResult;

use super::super::model::{SessionRecord, Transcript};
use super::super::records::{read_record, write_record};
use super::super::runtime::{Runtime, now_ms};
use super::super::transcript::{StreamTail, stream_path};
use super::seeded;
use super::status::settled;

const TUI_HISTORY_LIMIT: u64 = 256 * 1024;

pub struct Follower {
    runtime: Arc<dyn Runtime>,
    record: SessionRecord,
    session_dir: PathBuf,
    index_path: PathBuf,
    tail: StreamTail,
    transcript: Transcript,
}

impl Follower {
    pub(super) fn open(runtime: Arc<dyn Runtime>, record: SessionRecord, session_dir: PathBuf, index_path: PathBuf, indexed_to: u64, full_history: bool) -> NorthResult<Self> {
        let tail = StreamTail::open(stream_path(&record.host_dir), indexed_to);
        let transcript = seeded(&record);
        let mut follower = Self { runtime, record, session_dir, index_path: index_path.clone(), tail, transcript };
        let index_len = std::fs::metadata(&index_path).map_or(0, |metadata| metadata.len());
        let index_offset = if full_history { 0 } else { index_len.saturating_sub(TUI_HISTORY_LIMIT) };
        let mut index = StreamTail::open(index_path, index_offset);
        loop {
            let frames = index.frames()?;
            if frames.is_empty() {
                break;
            }
            follower.observe(frames);
        }
        follower.settle_status()?;
        Ok(follower)
    }

    pub fn poll(&mut self) -> NorthResult<bool> {
        let before = (self.transcript.entries.len(), self.transcript.status);
        self.refresh_record();
        let frames: Vec<_> = self.tail.frames()?.into_iter().filter(|frame| self.runtime.frame_conversation(frame) == self.record.conversation.as_deref()).collect();
        let got_frames = !frames.is_empty();
        self.observe(frames);
        self.persist_conversation()?;
        self.transcript.status = settled(&self.record, self.transcript.status);
        Ok(got_frames || before != (self.transcript.entries.len(), self.transcript.status))
    }

    fn observe(&mut self, frames: Vec<serde_json::Value>) {
        if !frames.is_empty() {
            self.transcript.last_activity_ms = Some(now_ms());
        }
        for frame in &frames {
            if let Some(quota) = self.runtime.quota(frame) {
                self.transcript.quota = Some(quota);
            }
            project(self.runtime.as_ref(), frame, &mut self.transcript);
        }
    }

    fn settle_status(&mut self) -> NorthResult<()> {
        if let Some(status) = super::status::latest_turn_status(&self.index_path, 0, self.record.conversation.as_deref(), &|frame| self.runtime.signals(frame))? {
            self.transcript.status = status;
        }
        self.transcript.status = settled(&self.record, self.transcript.status);
        Ok(())
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
        if let Some(status) = super::status::signal_status(&signal) {
            transcript.status = status;
        }
        if let Some(turn) = transcript.tracker.observe(signal, now_ms()) {
            transcript.turns.push(turn);
        }
    }
}
