use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{NorthError, NorthResult};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeliveryState {
    Pending,
    HostAccepted,
    Unknown,
    Queued,
    Started,
    Interrupted,
    Completed,
    Cancelled,
    Failed,
}

impl DeliveryState {
    fn rank(self) -> u8 {
        match self {
            Self::Pending => 0,
            Self::HostAccepted => 1,
            Self::Unknown => 2,
            Self::Queued => 3,
            Self::Started => 4,
            Self::Interrupted => 5,
            Self::Completed | Self::Cancelled | Self::Failed => 6,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Transition {
    pub(crate) uuid: String,
    pub(crate) state: DeliveryState,
    pub(crate) ts: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "UPPERCASE")]
pub(crate) enum Record {
    Submit { key: String, uuid: String, generation: u64, payload: Value, urgent: bool, ts: u64 },
    Host { uuid: String, host_seq: u64, ts: u64 },
    Observe {
        generation: u64,
        offset_after: u64,
        transitions: Vec<Transition>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Value>,
    },
    Gen { generation: u64, session_id: String, unit: String, resume: bool, ts: u64 },
    Recover { uuid: String, recovery_uuid: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Entry {
    pub(crate) seq: u64,
    #[serde(flatten)]
    pub(crate) record: Record,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Delivery {
    pub(crate) key: String,
    pub(crate) uuid: String,
    pub(crate) generation: u64,
    pub(crate) payload: Value,
    pub(crate) urgent: bool,
    pub(crate) state: DeliveryState,
    pub(crate) submitted_at: u64,
    pub(crate) host_seq: Option<u64>,
    pub(crate) queued_at: Option<u64>,
    pub(crate) seen_at: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Generation {
    pub(crate) session_id: String,
    pub(crate) unit: String,
    pub(crate) resume: bool,
    pub(crate) offset_after: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct JournalState {
    pub(crate) seq: u64,
    pub(crate) deliveries: BTreeMap<String, Delivery>,
    pub(crate) keys: BTreeMap<String, String>,
    pub(crate) generations: BTreeMap<u64, Generation>,
    pub(crate) recoveries: BTreeMap<String, String>,
}

impl JournalState {
    pub(crate) fn delivery(&self, uuid: &str) -> Option<&Delivery> {
        self.deliveries.get(uuid)
    }

    pub(crate) fn by_key(&self, key: &str) -> Option<&Delivery> {
        self.keys.get(key).and_then(|uuid| self.deliveries.get(uuid))
    }

    pub(crate) fn offset_after(&self, generation: u64) -> u64 {
        self.generations.get(&generation).map_or(0, |generation| generation.offset_after)
    }

    pub(crate) fn apply(&mut self, entry: &Entry) -> NorthResult<bool> {
        if entry.seq <= self.seq {
            return Err(damaged(format!("sequence {} does not follow {}", entry.seq, self.seq)));
        }
        self.seq = entry.seq;
        Ok(self.apply_record(&entry.record))
    }

    fn apply_record(&mut self, record: &Record) -> bool {
        match record {
            Record::Submit { key, uuid, generation, payload, urgent, ts } => {
                if let Some(delivery) = self.deliveries.get_mut(uuid) {
                    if delivery.key != *key || *generation <= delivery.generation { return false; }
                    delivery.generation = *generation;
                    return true;
                }
                self.keys.insert(key.clone(), uuid.clone());
                self.deliveries.insert(uuid.clone(), Delivery {
                    key: key.clone(),
                    uuid: uuid.clone(),
                    generation: *generation,
                    payload: payload.clone(),
                    urgent: *urgent,
                    state: DeliveryState::Pending,
                    submitted_at: *ts,
                    host_seq: None,
                    queued_at: None,
                    seen_at: None,
                });
                true
            }
            Record::Host { uuid, host_seq, ts } => {
                let Some(delivery) = self.deliveries.get_mut(uuid) else { return false };
                let changed = delivery.host_seq != Some(*host_seq);
                delivery.host_seq = Some(*host_seq);
                advance(delivery, DeliveryState::HostAccepted, *ts) || changed
            }
            Record::Observe { generation, offset_after, transitions, .. } => {
                let mut changed = false;
                if let Some(known) = self.generations.get_mut(generation)
                    && *offset_after > known.offset_after {
                    known.offset_after = *offset_after;
                    changed = true;
                }
                for transition in transitions {
                    if let Some(delivery) = self.deliveries.get_mut(&transition.uuid) {
                        changed |= advance(delivery, transition.state, transition.ts);
                    }
                }
                changed
            }
            Record::Gen { generation, session_id, unit, resume, .. } => {
                if self.generations.contains_key(generation) { return false; }
                self.generations.insert(*generation, Generation {
                    session_id: session_id.clone(),
                    unit: unit.clone(),
                    resume: *resume,
                    offset_after: 0,
                });
                true
            }
            Record::Recover { uuid, recovery_uuid } => {
                if self.recoveries.contains_key(uuid) { return false; }
                self.recoveries.insert(uuid.clone(), recovery_uuid.clone());
                true
            }
        }
    }

    fn would_change(&self, record: &Record) -> bool {
        self.clone().apply_record(record)
    }
}

fn advance(delivery: &mut Delivery, state: DeliveryState, ts: u64) -> bool {
    if state.rank() <= delivery.state.rank() { return false; }
    if state.rank() >= DeliveryState::Queued.rank() && delivery.queued_at.is_none() {
        delivery.queued_at = Some(ts);
    }
    if state.rank() >= DeliveryState::Started.rank() && delivery.seen_at.is_none() {
        delivery.seen_at = Some(ts);
    }
    delivery.state = state;
    true
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Submitted {
    pub(crate) delivery: Delivery,
    pub(crate) known: bool,
}

pub(crate) struct LeadJournal {
    file: File,
    _lock: File,
    state: JournalState,
}

impl LeadJournal {
    pub(crate) fn open(directory: &Path) -> NorthResult<Self> {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(directory)?;
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .mode(0o600).open(directory.join("journal.lock"))?;
        lock.try_lock().map_err(|error| NorthError::Configuration(format!(
            "Could not open the lead journal at {}: another North daemon may already have it open ({error})", directory.display()
        )))?;
        let mut file = OpenOptions::new().read(true).append(true).create(true)
            .mode(0o600).open(directory.join("journal.jsonl"))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let complete = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
        if complete < bytes.len() {
            file.set_len(complete as u64)?;
            file.sync_all()?;
        }
        let mut state = JournalState::default();
        for line in bytes[..complete].split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
            let entry: Entry = serde_json::from_slice(line)
                .map_err(|error| damaged(format!("unreadable record ({error})")))?;
            state.apply(&entry)?;
        }
        Ok(Self { file, _lock: lock, state })
    }

    pub(crate) fn state(&self) -> &JournalState {
        &self.state
    }

    fn append(&mut self, record: Record) -> NorthResult<Entry> {
        let entry = Entry { seq: self.state.seq + 1, record };
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.sync_data()?;
        self.state.apply(&entry)?;
        Ok(entry)
    }

    pub(crate) fn submit(&mut self, key: &str, uuid: &str, generation: u64, payload: Value, urgent: bool) -> NorthResult<Submitted> {
        if let Some(delivery) = self.state.by_key(key) {
            return Ok(Submitted { delivery: delivery.clone(), known: true });
        }
        if self.state.deliveries.contains_key(uuid) {
            return Err(NorthError::State(format!("delivery {uuid} already exists under another key")));
        }
        self.append(Record::Submit {
            key: key.into(), uuid: uuid.into(), generation, payload, urgent, ts: now_ms(),
        })?;
        Ok(Submitted { delivery: self.state.deliveries[uuid].clone(), known: false })
    }

    pub(crate) fn resend(&mut self, uuid: &str, generation: u64) -> NorthResult<Delivery> {
        let delivery = self.state.delivery(uuid)
            .ok_or_else(|| NorthError::State(format!("no delivery {uuid} to resend")))?.clone();
        if generation <= delivery.generation {
            return Err(NorthError::State(format!("delivery {uuid} already belongs to generation {}", delivery.generation)));
        }
        self.append(Record::Submit {
            key: delivery.key, uuid: uuid.into(), generation, payload: delivery.payload, urgent: delivery.urgent, ts: now_ms(),
        })?;
        Ok(self.state.deliveries[uuid].clone())
    }

    pub(crate) fn host(&mut self, uuid: &str, host_seq: u64) -> NorthResult<bool> {
        if !self.state.deliveries.contains_key(uuid) {
            return Err(NorthError::State(format!("no delivery {uuid} for host acceptance")));
        }
        self.append_if_changed(Record::Host { uuid: uuid.into(), host_seq, ts: now_ms() })
    }

    pub(crate) fn observe(&mut self, generation: u64, offset_after: u64, transitions: Vec<Transition>, usage: Option<Value>) -> NorthResult<bool> {
        if !self.state.generations.contains_key(&generation) {
            return Err(NorthError::State(format!("no generation {generation} to observe")));
        }
        self.append_if_changed(Record::Observe { generation, offset_after, transitions, usage })
    }

    pub(crate) fn generation(&mut self, generation: u64, session_id: &str, unit: &str, resume: bool) -> NorthResult<bool> {
        if self.state.generations.keys().next_back().is_some_and(|last| generation <= *last) {
            return Err(NorthError::State(format!("generation {generation} does not follow the journal")));
        }
        self.append_if_changed(Record::Gen {
            generation, session_id: session_id.into(), unit: unit.into(), resume, ts: now_ms(),
        })
    }

    pub(crate) fn recover(&mut self, uuid: &str, recovery_uuid: &str) -> NorthResult<bool> {
        self.append_if_changed(Record::Recover { uuid: uuid.into(), recovery_uuid: recovery_uuid.into() })
    }

    fn append_if_changed(&mut self, record: Record) -> NorthResult<bool> {
        if !self.state.would_change(&record) { return Ok(false); }
        self.append(record)?;
        Ok(true)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TranscriptPresence {
    Present,
    Absent,
    Unreadable,
}

pub(crate) trait TranscriptLookup {
    fn lookup(&self, uuid: &str) -> TranscriptPresence;
}

impl<F: Fn(&str) -> TranscriptPresence> TranscriptLookup for F {
    fn lookup(&self, uuid: &str) -> TranscriptPresence {
        self(uuid)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GenerationEndAction {
    Resend { uuid: String },
    Recover { uuid: String, transitions: Vec<DeliveryState>, recovery_key: String },
    Unknown { uuid: String },
}

pub(crate) fn recovery_key(uuid: &str) -> String {
    format!("recover:{uuid}")
}

pub(crate) fn classify_on_generation_end(
    state: &JournalState,
    generation: u64,
    old_process_dead: bool,
    transcript: &impl TranscriptLookup,
) -> Vec<GenerationEndAction> {
    let recover = |uuid: &str, transitions: Vec<DeliveryState>| GenerationEndAction::Recover {
        uuid: uuid.into(), transitions, recovery_key: recovery_key(uuid),
    };
    state.deliveries.values().filter(|delivery| delivery.generation == generation).filter_map(|delivery| {
        let uuid = delivery.uuid.as_str();
        match delivery.state {
            DeliveryState::Pending => Some(GenerationEndAction::Resend { uuid: uuid.into() }),
            DeliveryState::HostAccepted | DeliveryState::Queued => match transcript.lookup(uuid) {
                TranscriptPresence::Present => Some(recover(uuid, vec![DeliveryState::Started, DeliveryState::Interrupted])),
                TranscriptPresence::Absent if old_process_dead => Some(GenerationEndAction::Resend { uuid: uuid.into() }),
                TranscriptPresence::Absent | TranscriptPresence::Unreadable => Some(GenerationEndAction::Unknown { uuid: uuid.into() }),
            },
            DeliveryState::Started => Some(recover(uuid, vec![DeliveryState::Interrupted])),
            DeliveryState::Interrupted if !state.recoveries.contains_key(uuid) => Some(recover(uuid, Vec::new())),
            _ => None,
        }
    }).collect()
}

fn damaged(detail: String) -> NorthError {
    NorthError::State(format!("lead journal is damaged and has been preserved: {detail}"))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn transition(uuid: &str, state: DeliveryState, ts: u64) -> Transition {
        Transition { uuid: uuid.into(), state, ts }
    }

    fn journal_lines(directory: &Path) -> Vec<String> {
        fs::read_to_string(directory.join("journal.jsonl")).unwrap().lines().map(str::to_owned).collect()
    }

    fn seeded(directory: &Path) -> LeadJournal {
        let mut journal = LeadJournal::open(directory).unwrap();
        journal.generation(1, "session", "north-lead-x-g1", false).unwrap();
        for uuid in ["pending", "accepted", "queued", "started", "done", "lost"] {
            journal.submit(&format!("ev:{uuid}:g1"), uuid, 1, json!({"text": uuid}), false).unwrap();
        }
        for (seq, uuid) in ["accepted", "queued", "started", "done", "lost"].into_iter().enumerate() {
            journal.host(uuid, seq as u64 + 1).unwrap();
        }
        journal.observe(1, 100, vec![
            transition("queued", DeliveryState::Queued, 10),
            transition("started", DeliveryState::Queued, 11),
            transition("started", DeliveryState::Started, 12),
            transition("done", DeliveryState::Started, 13),
            transition("done", DeliveryState::Completed, 14),
        ], Some(json!({"input_tokens": 3}))).unwrap();
        journal
    }

    fn state_of(journal: &LeadJournal, uuid: &str) -> DeliveryState {
        journal.state().delivery(uuid).unwrap().state
    }

    #[test]
    fn lead_journal_replays_to_expected_states_including_unknown_and_interrupted() {
        let directory = tempfile::tempdir().unwrap();
        let mut journal = seeded(directory.path());
        journal.observe(1, 100, vec![
            transition("started", DeliveryState::Interrupted, 20),
            transition("lost", DeliveryState::Unknown, 21),
            transition("done", DeliveryState::Started, 22),
        ], None).unwrap();
        let before = journal.state().clone();
        drop(journal);
        let journal = reopen(directory.path());
        assert_eq!(journal.state(), &before);
        assert_eq!(state_of(&journal, "pending"), DeliveryState::Pending);
        assert_eq!(state_of(&journal, "accepted"), DeliveryState::HostAccepted);
        assert_eq!(state_of(&journal, "queued"), DeliveryState::Queued);
        assert_eq!(state_of(&journal, "started"), DeliveryState::Interrupted);
        assert_eq!(state_of(&journal, "done"), DeliveryState::Completed);
        assert_eq!(state_of(&journal, "lost"), DeliveryState::Unknown);
        let started = journal.state().delivery("started").unwrap();
        assert_eq!((started.queued_at, started.seen_at), (Some(11), Some(12)));
        assert_eq!(journal.state().delivery("lost").unwrap().seen_at, None);
        assert_eq!(journal.state().offset_after(1), 100);
        let entries: Vec<Entry> = journal_lines(directory.path()).iter().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert!(entries.windows(2).all(|pair| pair[0].seq < pair[1].seq));
        assert_eq!(entries.first().unwrap().seq, 1);
    }

    #[test]
    fn lead_journal_repairs_torn_suffix_before_append() {
        let directory = tempfile::tempdir().unwrap();
        let journal = seeded(directory.path());
        let complete = journal_lines(directory.path());
        let before = journal.state().clone();
        drop(journal);
        let path = directory.path().join("journal.jsonl");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(br#"{"seq":99,"type":"HOST","uuid":"pend"#).unwrap();
        drop(file);

        let mut journal = reopen(directory.path());
        assert_eq!(journal.state(), &before);
        journal.host("pending", 9).unwrap();
        drop(journal);

        let journal = reopen(directory.path());
        let lines = journal_lines(directory.path());
        assert_eq!(lines[..complete.len()], complete[..]);
        assert_eq!(lines.len(), complete.len() + 1);
        let last: Entry = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(last.seq, before.seq + 1);
        assert!(matches!(last.record, Record::Host { ref uuid, host_seq: 9, .. } if uuid == "pending"));
        assert_eq!(state_of(&journal, "pending"), DeliveryState::HostAccepted);
        assert!(fs::read(&path).unwrap().ends_with(b"\n"));
    }

    fn reopen(directory: &Path) -> LeadJournal {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match LeadJournal::open(directory) {
                Ok(journal) => return journal,
                Err(NorthError::Configuration(_)) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("{error:?}"),
            }
        }
    }

    #[test]
    fn lead_journal_second_opener_fails_on_flock() {
        let directory = tempfile::tempdir().unwrap();
        let journal = LeadJournal::open(directory.path()).unwrap();
        assert!(LeadJournal::open(directory.path()).is_err());
        drop(journal);
        reopen(directory.path());
    }

    #[test]
    fn lead_journal_known_key_returns_original_without_appending() {
        let directory = tempfile::tempdir().unwrap();
        let mut journal = seeded(directory.path());
        let original = journal.state().by_key("ev:started:g1").unwrap().clone();
        let lines = journal_lines(directory.path());
        let again = journal.submit("ev:started:g1", "a-new-uuid", 1, json!({"text": "other"}), true).unwrap();
        assert!(again.known);
        assert_eq!(again.delivery, original);
        assert_eq!(again.delivery.uuid, "started");
        assert_eq!(journal_lines(directory.path()), lines);
        assert!(journal.submit("ev:other:g1", "started", 1, json!({}), false).is_err());
        assert_eq!(journal_lines(directory.path()), lines);
        drop(journal);
        let mut journal = reopen(directory.path());
        assert_eq!(journal.submit("ev:started:g1", "x", 1, json!({}), false).unwrap().delivery, original);
        assert_eq!(journal_lines(directory.path()), lines);
    }

    #[test]
    fn lead_journal_observe_replayed_twice_changes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let mut journal = seeded(directory.path());
        let observed = vec![
            transition("accepted", DeliveryState::Queued, 30),
            transition("accepted", DeliveryState::Started, 31),
        ];
        assert!(journal.observe(1, 200, observed.clone(), None).unwrap());
        let state = journal.state().clone();
        let lines = journal_lines(directory.path());
        assert!(!journal.observe(1, 200, observed.clone(), None).unwrap());
        assert!(!journal.observe(1, 150, vec![transition("accepted", DeliveryState::HostAccepted, 40)], None).unwrap());
        assert_eq!(journal.state(), &state);
        assert_eq!(journal_lines(directory.path()), lines);
        drop(journal);

        let duplicate: Entry = serde_json::from_str(lines.last().unwrap()).unwrap();
        let replayed = Entry { seq: duplicate.seq + 1, record: duplicate.record };
        let mut file = OpenOptions::new().append(true).open(directory.path().join("journal.jsonl")).unwrap();
        file.write_all(format!("{}\n", serde_json::to_string(&replayed).unwrap()).as_bytes()).unwrap();
        drop(file);
        let journal = reopen(directory.path());
        let mut expected = state;
        expected.seq += 1;
        assert_eq!(journal.state(), &expected);
        let accepted = journal.state().delivery("accepted").unwrap();
        assert_eq!((accepted.state, accepted.queued_at, accepted.seen_at), (DeliveryState::Started, Some(30), Some(31)));
    }

    #[test]
    fn lead_journal_classifies_on_generation_end() {
        let directory = tempfile::tempdir().unwrap();
        let mut journal = seeded(directory.path());
        journal.submit("ev:unread:g1", "unread", 1, json!({}), false).unwrap();
        journal.host("unread", 7).unwrap();
        journal.submit("ev:already:g1", "already", 1, json!({}), false).unwrap();
        journal.observe(1, 300, vec![transition("already", DeliveryState::Interrupted, 50)], None).unwrap();
        journal.submit("ev:recovered:g1", "recovered", 1, json!({}), false).unwrap();
        journal.observe(1, 300, vec![transition("recovered", DeliveryState::Interrupted, 51)], None).unwrap();
        journal.recover("recovered", "recovery-1").unwrap();
        journal.generation(2, "session", "north-lead-x-g2", true).unwrap();
        journal.submit("ev:next:g2", "next", 2, json!({}), false).unwrap();

        let transcript = |uuid: &str| match uuid {
            "accepted" => TranscriptPresence::Present,
            "unread" => TranscriptPresence::Unreadable,
            _ => TranscriptPresence::Absent,
        };
        let actions = classify_on_generation_end(journal.state(), 1, true, &transcript);
        let by_uuid: BTreeMap<String, GenerationEndAction> = actions.into_iter().map(|action| {
            let uuid = match &action {
                GenerationEndAction::Resend { uuid } | GenerationEndAction::Recover { uuid, .. } | GenerationEndAction::Unknown { uuid } => uuid.clone(),
            };
            (uuid, action)
        }).collect();
        assert_eq!(by_uuid.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from(["accepted", "already", "lost", "pending", "queued", "started", "unread"]));
        assert_eq!(by_uuid["pending"], GenerationEndAction::Resend { uuid: "pending".into() });
        assert_eq!(by_uuid["accepted"], GenerationEndAction::Recover {
            uuid: "accepted".into(),
            transitions: vec![DeliveryState::Started, DeliveryState::Interrupted],
            recovery_key: "recover:accepted".into(),
        });
        assert_eq!(by_uuid["queued"], GenerationEndAction::Resend { uuid: "queued".into() });
        assert_eq!(by_uuid["unread"], GenerationEndAction::Unknown { uuid: "unread".into() });
        assert_eq!(by_uuid["started"], GenerationEndAction::Recover {
            uuid: "started".into(), transitions: vec![DeliveryState::Interrupted], recovery_key: "recover:started".into(),
        });
        assert_eq!(by_uuid["already"], GenerationEndAction::Recover {
            uuid: "already".into(), transitions: Vec::new(), recovery_key: "recover:already".into(),
        });

        let alive = classify_on_generation_end(journal.state(), 1, false, &transcript);
        assert!(alive.contains(&GenerationEndAction::Unknown { uuid: "queued".into() }));
        assert!(alive.contains(&GenerationEndAction::Unknown { uuid: "lost".into() }));

        let lines = journal_lines(directory.path());
        journal.resend("pending", 2).unwrap();
        assert_eq!(journal_lines(directory.path()).len(), lines.len() + 1);
        assert_eq!(journal.state().delivery("pending").unwrap().generation, 2);
        assert!(classify_on_generation_end(journal.state(), 1, true, &transcript)
            .iter().all(|action| action != &GenerationEndAction::Resend { uuid: "pending".into() }));
        assert!(journal.resend("pending", 2).is_err());
    }
}
