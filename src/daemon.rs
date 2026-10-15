use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::claude_stream::{self, DeliveryState as Lifecycle, StreamFrame};
use crate::error::NorthResult;
use crate::lead_journal::{DeliveryState, JournalState, LeadJournal, Transition};
use crate::foundation::clock::Timestamp;

pub(crate) fn stream_path(lead_dir: &Path, generation: u64) -> PathBuf {
    lead_dir.join(format!("g{generation}")).join("stream.jsonl")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Observed {
    pub(crate) lines: usize,
    pub(crate) skipped: usize,
    pub(crate) offset_after: u64,
    pub(crate) transitions: Vec<Transition>,
}

pub(crate) struct StreamTail {
    path: PathBuf,
    generation: u64,
    offset: u64,
    partial: Vec<u8>,
}

impl StreamTail {
    pub(crate) fn open(lead_dir: &Path, generation: u64, journal: &JournalState) -> Self {
        Self { path: stream_path(lead_dir, generation), generation, offset: journal.offset_after(generation), partial: Vec::new() }
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn poll(&mut self, journal: &mut LeadJournal) -> NorthResult<Observed> {
        self.read_new()?;
        let complete = self.partial.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
        let mut observed = Observed { offset_after: self.offset, ..Observed::default() };
        if complete == 0 { return Ok(observed); }
        let mut current: BTreeMap<String, DeliveryState> = BTreeMap::new();
        let ts = Timestamp::now().unix_millis() as u64;
        for line in self.partial[..complete].split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
            observed.lines += 1;
            let Ok(frame) = std::str::from_utf8(line).map_err(drop).and_then(|line| claude_stream::decode_frame(line).map_err(drop)) else {
                observed.skipped += 1;
                continue;
            };
            for (uuid, state) in frame_transitions(&frame) {
                let Some(delivery) = journal.state().delivery(uuid) else { continue };
                let known = current.get(uuid).copied().unwrap_or(delivery.state);
                if state.rank() <= known.rank() { continue; }
                current.insert(uuid.into(), state);
                observed.transitions.push(Transition { uuid: uuid.into(), state, ts });
            }
        }
        observed.offset_after = self.offset + complete as u64;
        journal.observe(self.generation, observed.offset_after, observed.transitions.clone(), None)?;
        self.partial.drain(..complete);
        self.offset = observed.offset_after;
        Ok(observed)
    }

    fn read_new(&mut self) -> io::Result<()> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        file.seek(SeekFrom::Start(self.offset + self.partial.len() as u64))?;
        file.read_to_end(&mut self.partial)?;
        Ok(())
    }
}

pub(crate) fn frame_transitions(frame: &StreamFrame) -> Vec<(&str, DeliveryState)> {
    match frame {
        StreamFrame::CommandLifecycle(lifecycle) => match lifecycle.state {
            Lifecycle::Queued => vec![(lifecycle.command_uuid.as_str(), DeliveryState::Queued)],
            Lifecycle::AcceptedIntoTurn => vec![(lifecycle.command_uuid.as_str(), DeliveryState::Started)],
            Lifecycle::Completed => vec![(lifecycle.command_uuid.as_str(), DeliveryState::Completed)],
            Lifecycle::Cancelled => vec![(lifecycle.command_uuid.as_str(), DeliveryState::Cancelled)],
            Lifecycle::Unknown => Vec::new(),
        },
        StreamFrame::User(user) if user.is_replay => user.uuid.as_deref().map(|uuid| (uuid, DeliveryState::Started)).into_iter().collect(),
        StreamFrame::Result(result) => {
            let state = if !result.is_error {
                DeliveryState::Completed
            } else if result.terminal_reason.as_deref().is_some_and(|reason| reason.starts_with("aborted")) {
                DeliveryState::Cancelled
            } else {
                DeliveryState::Failed
            };
            result.user_message_uuids.iter().map(|uuid| (uuid.as_str(), state)).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tail {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    use serde_json::json;

    use super::*;
    use crate::lead_journal::{Entry, Record};

    const FOLD: &str = include_str!("fixtures/claude-stream/fold.jsonl");
    const FIRST: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND: &str = "22222222-2222-4222-8222-222222222222";

    fn lead(directory: &Path) -> (PathBuf, LeadJournal) {
        let lead_dir = directory.join("lead");
        let mut journal = LeadJournal::open(&lead_dir).unwrap();
        journal.generation(1, "session", "north-lead-x-g1", false).unwrap();
        for uuid in [FIRST, SECOND] {
            journal.submit(&format!("ev:{uuid}:g1"), uuid, 1, json!({"text": uuid}), false).unwrap();
        }
        fs::create_dir_all(lead_dir.join("g1")).unwrap();
        (lead_dir, journal)
    }

    fn append(lead_dir: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new().create(true).append(true).open(stream_path(lead_dir, 1)).unwrap();
        file.write_all(bytes).unwrap();
    }

    fn pairs(transitions: &[Transition]) -> Vec<(String, DeliveryState)> {
        transitions.iter().map(|transition| (transition.uuid.clone(), transition.state)).collect()
    }

    fn expected() -> Vec<(String, DeliveryState)> {
        [
            (FIRST, DeliveryState::Queued),
            (FIRST, DeliveryState::Started),
            (SECOND, DeliveryState::Queued),
            (FIRST, DeliveryState::Cancelled),
            (SECOND, DeliveryState::Started),
            (SECOND, DeliveryState::Completed),
        ].into_iter().map(|(uuid, state)| (uuid.to_owned(), state)).collect()
    }

    fn lines(count: usize) -> String {
        FOLD.split_inclusive('\n').take(count).collect()
    }

    #[test]
    fn tail_seven_byte_chunks_observe_the_same_transitions_as_whole_lines() {
        let whole_dir = tempfile::tempdir().unwrap();
        let (lead_dir, mut journal) = lead(whole_dir.path());
        append(&lead_dir, FOLD.as_bytes());
        let mut tail = StreamTail::open(&lead_dir, 1, journal.state());
        let whole = pairs(&tail.poll(&mut journal).unwrap().transitions);

        let chunked_dir = tempfile::tempdir().unwrap();
        let (lead_dir, mut journal) = lead(chunked_dir.path());
        let mut tail = StreamTail::open(&lead_dir, 1, journal.state());
        let mut chunked = Vec::new();
        for chunk in FOLD.as_bytes().chunks(7) {
            append(&lead_dir, chunk);
            chunked.extend(pairs(&tail.poll(&mut journal).unwrap().transitions));
        }
        assert_eq!(whole, expected());
        assert_eq!(chunked, whole);
        assert_eq!(journal.state().delivery(FIRST).unwrap().state, DeliveryState::Cancelled);
        assert_eq!(journal.state().delivery(SECOND).unwrap().state, DeliveryState::Completed);
    }

    #[test]
    fn tail_offset_never_passes_a_partial_line() {
        let directory = tempfile::tempdir().unwrap();
        let (lead_dir, mut journal) = lead(directory.path());
        let complete = lines(3);
        let fourth = FOLD.split_inclusive('\n').nth(3).unwrap();
        append(&lead_dir, complete.as_bytes());
        append(&lead_dir, &fourth.as_bytes()[..fourth.len() / 2]);
        let mut tail = StreamTail::open(&lead_dir, 1, journal.state());
        let observed = tail.poll(&mut journal).unwrap();
        assert_eq!(observed.lines, 3);
        assert_eq!(observed.offset_after, complete.len() as u64);
        assert_eq!(journal.state().offset_after(1), complete.len() as u64);
    }

    #[test]
    fn tail_restart_from_offset_after_repeats_no_transition() {
        let directory = tempfile::tempdir().unwrap();
        let (lead_dir, mut journal) = lead(directory.path());
        append(&lead_dir, lines(9).as_bytes());
        let mut tail = StreamTail::open(&lead_dir, 1, journal.state());
        let mut seen = pairs(&tail.poll(&mut journal).unwrap().transitions);
        drop(tail);
        drop(journal);

        append(&lead_dir, FOLD[lines(9).len()..].as_bytes());
        let mut journal = LeadJournal::open(&lead_dir).unwrap();
        let mut tail = StreamTail::open(&lead_dir, 1, journal.state());
        assert_eq!(tail.offset(), lines(9).len() as u64);
        let observed = tail.poll(&mut journal).unwrap();
        assert_eq!(observed.lines, FOLD.lines().count() - 9);
        seen.extend(pairs(&observed.transitions));
        assert_eq!(seen, expected());
        drop(journal);

        let journaled: Vec<(String, DeliveryState)> = fs::read_to_string(lead_dir.join("journal.jsonl")).unwrap().lines()
            .map(|line| serde_json::from_str::<Entry>(line).unwrap().record)
            .filter_map(|record| match record { Record::Observe { transitions, .. } => Some(pairs(&transitions)), _ => None })
            .flatten().collect();
        assert_eq!(journaled, expected());
    }

    #[test]
    fn tail_invalid_json_line_is_skipped_counted_and_passed() {
        let directory = tempfile::tempdir().unwrap();
        let (lead_dir, mut journal) = lead(directory.path());
        let stream = format!(
            "{}\n{{\"type\":\"command_lifecycle\",\"command_uu\n{}\n",
            json!({"type": "command_lifecycle", "command_uuid": FIRST, "state": "queued"}),
            json!({"type": "command_lifecycle", "command_uuid": FIRST, "state": "started"}),
        );
        append(&lead_dir, stream.as_bytes());
        let mut tail = StreamTail::open(&lead_dir, 1, journal.state());
        let observed = tail.poll(&mut journal).unwrap();
        assert_eq!((observed.lines, observed.skipped), (3, 1));
        assert_eq!(observed.offset_after, stream.len() as u64);
        assert_eq!(pairs(&observed.transitions), [(FIRST.to_owned(), DeliveryState::Queued), (FIRST.to_owned(), DeliveryState::Started)]);
        assert_eq!(journal.state().offset_after(1), stream.len() as u64);
    }

    #[test]
    fn tail_error_result_fails_the_turn() {
        let directory = tempfile::tempdir().unwrap();
        let (lead_dir, mut journal) = lead(directory.path());
        let stream = format!("{}\n", json!({
            "type": "result", "subtype": "error_max_turns", "is_error": true,
            "terminal_reason": "max_turns", "user_message_uuids": [FIRST],
        }));
        append(&lead_dir, stream.as_bytes());
        let mut tail = StreamTail::open(&lead_dir, 1, journal.state());
        assert_eq!(pairs(&tail.poll(&mut journal).unwrap().transitions), [(FIRST.to_owned(), DeliveryState::Failed)]);
        assert_eq!(journal.state().delivery(FIRST).unwrap().state, DeliveryState::Failed);
    }
}
