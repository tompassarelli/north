use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};

use super::super::model::{Account, SessionRecord, Status, Transcript};
use super::super::records::{read_record, write_record};
use super::super::runtime::{Runtime, now_ms};
use super::super::transcript::{StreamTail, stream_path};
use super::streams::seeded;
use super::status::settled;
use super::Fleet;

const TUI_HISTORY_LIMIT: u64 = 256 * 1024;

pub struct Follower {
    runtime: Arc<dyn Runtime>,
    record: SessionRecord,
    session_dir: PathBuf,
    tail: Option<StreamTail>,
    window: Option<Window>,
    transcript: Transcript,
}

/// An adopted window's conversation, read from the client's own file for it; the file is found again whenever the name moves to another conversation.
struct Window {
    accounts: Vec<Account>,
    status: Status,
    full_history: bool,
    conversation: Option<String>,
    log: Option<StreamTail>,
}

impl Follower {
    pub(super) fn open(runtime: Arc<dyn Runtime>, record: SessionRecord, session_dir: PathBuf, index_path: PathBuf, indexed_to: u64, full_history: bool) -> NorthResult<Self> {
        let tail = Some(StreamTail::open(stream_path(&record.host_dir), indexed_to));
        let transcript = seeded(&record);
        let mut follower = Self { runtime, record, session_dir, tail, window: None, transcript };
        let index_len = std::fs::metadata(&index_path).map_or(0, |metadata| metadata.len());
        let index_offset = if full_history { 0 } else { index_len.saturating_sub(TUI_HISTORY_LIMIT) };
        let mut index = StreamTail::open(index_path.clone(), index_offset);
        loop {
            let frames = index.frames()?;
            if frames.is_empty() {
                break;
            }
            follower.observe(frames);
        }
        follower.settle_status(&index_path)?;
        Ok(follower)
    }

    pub(super) fn window(runtime: Arc<dyn Runtime>, record: SessionRecord, session_dir: PathBuf, status: Status, accounts: Vec<Account>, full_history: bool) -> NorthResult<Self> {
        let mut transcript = seeded(&record);
        transcript.status = status;
        let window = Window { accounts, status, full_history, conversation: record.conversation.clone(), log: None };
        let mut follower = Self { runtime, record, session_dir, tail: None, window: Some(window), transcript };
        let mut frames = follower.next_frames()?;
        while !frames.is_empty() {
            follower.observe(frames);
            frames = follower.next_frames()?;
        }
        Ok(follower)
    }

    pub fn poll(&mut self) -> NorthResult<bool> {
        self.refresh_record();
        if self.record.conversation.is_some() {
            self.transcript.conversation.clone_from(&self.record.conversation);
        }
        let before = (self.transcript.entries.len(), self.transcript.status);
        let frames = self.next_frames()?;
        let got_frames = !frames.is_empty();
        self.observe(frames);
        self.persist_conversation()?;
        self.transcript.status = settled(&self.record, self.transcript.status);
        Ok(got_frames || before != (self.transcript.entries.len(), self.transcript.status))
    }

    fn next_frames(&mut self) -> NorthResult<Vec<Value>> {
        if let Some(window) = self.window.as_mut() {
            if window.follow(self.record.conversation.as_deref()) {
                self.transcript.entries.clear();
                self.transcript.partial_reply = None;
            }
            return window.frames(self.runtime.as_ref());
        }
        let Some(tail) = self.tail.as_mut() else { return Ok(Vec::new()) };
        let conversation = self.record.conversation.as_deref();
        let runtime = self.runtime.as_ref();
        Ok(tail.frames()?.into_iter().filter(|frame| runtime.frame_conversation(frame) == conversation).collect())
    }

    fn observe(&mut self, frames: Vec<Value>) {
        if !frames.is_empty() {
            self.transcript.last_activity_ms = Some(now_ms());
        }
        for frame in &frames {
            if let Some(quota) = self.runtime.quota(frame) {
                self.transcript.quota = Some(quota);
            }
            project(self.runtime.as_ref(), frame, &mut self.transcript);
        }
        if let Some(window) = &self.window {
            self.transcript.status = window.status;
        }
    }

    fn settle_status(&mut self, index_path: &Path) -> NorthResult<()> {
        if let Some(status) = super::status::latest_turn_status(index_path, 0, self.record.conversation.as_deref(), &|frame| self.runtime.signals(frame))? {
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

impl Window {
    /// Points the window at `conversation`, reporting whether that moves it off the conversation it read.
    fn follow(&mut self, conversation: Option<&str>) -> bool {
        if self.conversation.as_deref() == conversation {
            return false;
        }
        self.conversation = conversation.map(str::to_owned);
        self.log = None;
        true
    }

    fn frames(&mut self, runtime: &dyn Runtime) -> NorthResult<Vec<Value>> {
        if self.log.is_none() {
            self.log = self.conversation.as_deref().and_then(|conversation| runtime.conversation_log(&self.accounts, conversation)).map(|path| {
                let length = fs::metadata(&path).map_or(0, |metadata| metadata.len());
                StreamTail::open(path, if self.full_history { 0 } else { length.saturating_sub(TUI_HISTORY_LIMIT) })
            });
        }
        match self.log.as_mut() {
            Some(log) => Ok(log.frames()?),
            None => Ok(Vec::new()),
        }
    }
}

impl Fleet {
    pub fn follow(&self, name: &str) -> NorthResult<Follower> {
        self.follow_with_history(name, true)
    }

    /// A retired session's transcript read from its archive: the reaper moves a session out of the registry once its report is final, and dispatch reads that report there.
    pub fn follow_retired(&self, name: &str) -> NorthResult<Follower> {
        let (dir, mut record) = self.retired_session(name).ok_or_else(|| NorthError::Usage(format!("no session named {name}")))?;
        if record.host_dir == self.root.join(name) {
            record.host_dir = dir.clone();
        }
        let (path, indexed_to) = self.stream_index_among(&record, || Ok(vec![record.clone()]))?;
        Follower::open(self.runtime(&record)?, record, dir, path, indexed_to, true)
    }

    pub fn follow_recent(&self, name: &str) -> NorthResult<Follower> {
        self.follow_with_history(name, false)
    }

    fn follow_with_history(&self, name: &str, full_history: bool) -> NorthResult<Follower> {
        let record = self.record(name)?;
        if record.adopted() {
            let runtime = self.runtime(&record)?;
            let accounts = self.accounts().into_iter().filter(|account| account.provider == runtime.provider()).collect();
            let status = self.window_status(&record);
            return Follower::window(runtime, record, self.root.join(name), status, accounts, full_history);
        }
        let (path, indexed_to) = self.stream_index(&record)?;
        Follower::open(self.runtime(&record)?, record, self.root.join(name), path, indexed_to, full_history)
    }
}

fn project(runtime: &dyn Runtime, frame: &Value, transcript: &mut Transcript) {
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
