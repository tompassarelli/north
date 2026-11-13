use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};

use super::streams::seeded;
use crate::fleet::host::alive;
use crate::fleet::model::{EntryKind, SessionRecord, Status, Transcript};
use crate::fleet::runtime::Runtime;
use crate::fleet::transcript::{StreamTail, stream_path};

const OPEN_WAIT: Duration = Duration::from_secs(30);
const OPEN_POLL: Duration = Duration::from_millis(100);
const SETTLE_WAIT: Duration = Duration::from_secs(15);
const SETTLE_POLL: Duration = Duration::from_millis(50);

pub(super) fn await_settled(runtime: &dyn Runtime, record: &SessionRecord, mut transcript: Transcript, offset: u64) -> NorthResult<Transcript> {
    let mut tail = StreamTail::open(stream_path(&record.host_dir), offset);
    let deadline = Instant::now() + SETTLE_WAIT;
    while transcript.status == Status::Working && Instant::now() < deadline {
        std::thread::sleep(SETTLE_POLL);
        for frame in tail.frames()? {
            runtime.observe(&frame, &mut transcript);
        }
    }
    Ok(transcript)
}

pub(super) fn await_reply(runtime: &dyn Runtime, record: &SessionRecord, offset: u64, question: &str, timeout: Duration) -> NorthResult<Option<String>> {
    let mut tail = StreamTail::open(stream_path(&record.host_dir), offset);
    let mut transcript = seeded(record);
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        for frame in tail.frames()? {
            runtime.observe(&frame, &mut transcript);
        }
        if let Some(reply) = reply_to(&transcript, question) {
            return Ok(Some(reply));
        }
        std::thread::sleep(SETTLE_POLL);
    }
    Ok(None)
}

pub(super) fn reply_to(transcript: &Transcript, question: &str) -> Option<String> {
    let asked = transcript.entries.iter().position(|entry| entry.kind == EntryKind::User && entry.text.contains(question))?;
    transcript.entries[asked..].iter().find(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.clone())
}

pub(super) fn await_conversation(runtime: &dyn Runtime, record: &SessionRecord, offset: u64) -> NorthResult<String> {
    let mut tail = StreamTail::open(stream_path(&record.host_dir), offset);
    let mut transcript = seeded(record);
    transcript.conversation = None;
    let deadline = Instant::now() + OPEN_WAIT;
    loop {
        for frame in tail.frames()? {
            runtime.observe(&frame, &mut transcript);
        }
        if let Some(conversation) = transcript.conversation.take() {
            return Ok(conversation);
        }
        if Instant::now() > deadline || !record.host_pid.is_some_and(alive) {
            return Err(NorthError::State(format!("{} did not open a conversation", record.name)));
        }
        std::thread::sleep(OPEN_POLL);
    }
}
