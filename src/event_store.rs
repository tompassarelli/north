use crate::error::{NorthError, NorthResult};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use std::path::{Path, PathBuf};

pub(crate) const KINDS: [&str; 8] = [
    "KICKOFF",
    "STUCK",
    "LANDED",
    "QUESTION",
    "LAND-READY",
    "RECYCLE-READY",
    "ANSWER",
    "LEARN",
];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS agent_events(
  id INTEGER PRIMARY KEY,
  time TEXT NOT NULL,
  item TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('KICKOFF','STUCK','LANDED','QUESTION','LAND-READY','RECYCLE-READY','ANSWER','LEARN')),
  from_node TEXT NOT NULL,
  to_node TEXT NOT NULL,
  repo TEXT,
  lane TEXT,
  sha TEXT,
  body TEXT,
  packet TEXT,
  key TEXT NOT NULL UNIQUE,
  root_event INTEGER,
  answer INTEGER,
  answered_by INTEGER,
  attempted_at TEXT,
  delivered_at TEXT,
  last_error TEXT,
  parent_escalated_at TEXT,
  proxy_escalated_at TEXT,
  resend_at TEXT,
  successor TEXT
) STRICT;
";

const COLUMNS: &str = "id,time,item,kind,from_node,to_node,repo,lane,sha,body,packet,key,root_event,answer,answered_by,attempted_at,delivered_at,last_error,parent_escalated_at,proxy_escalated_at,resend_at,successor";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NewEvent {
    pub(crate) kind: String,
    pub(crate) item: Option<String>,
    pub(crate) from_node: String,
    pub(crate) to_node: String,
    pub(crate) repo: Option<String>,
    pub(crate) lane: Option<String>,
    pub(crate) sha: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) packet: Option<String>,
    pub(crate) key: Option<String>,
    pub(crate) root_event: Option<i64>,
    pub(crate) answer: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Event {
    pub(crate) id: i64,
    pub(crate) time: String,
    pub(crate) item: Option<String>,
    pub(crate) kind: String,
    pub(crate) from_node: String,
    pub(crate) to_node: String,
    pub(crate) repo: Option<String>,
    pub(crate) lane: Option<String>,
    pub(crate) sha: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) packet: Option<String>,
    pub(crate) key: String,
    pub(crate) root_event: Option<i64>,
    pub(crate) answer: Option<i64>,
    pub(crate) answered_by: Option<i64>,
    pub(crate) attempted_at: Option<String>,
    pub(crate) delivered_at: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) parent_escalated_at: Option<String>,
    pub(crate) proxy_escalated_at: Option<String>,
    pub(crate) resend_at: Option<String>,
    pub(crate) successor: Option<String>,
}

impl Event {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            time: row.get(1)?,
            item: row.get(2)?,
            kind: row.get(3)?,
            from_node: row.get(4)?,
            to_node: row.get(5)?,
            repo: row.get(6)?,
            lane: row.get(7)?,
            sha: row.get(8)?,
            body: row.get(9)?,
            packet: row.get(10)?,
            key: row.get(11)?,
            root_event: row.get(12)?,
            answer: row.get(13)?,
            answered_by: row.get(14)?,
            attempted_at: row.get(15)?,
            delivered_at: row.get(16)?,
            last_error: row.get(17)?,
            parent_escalated_at: row.get(18)?,
            proxy_escalated_at: row.get(19)?,
            resend_at: row.get(20)?,
            successor: row.get(21)?,
        })
    }

    fn payload(&self) -> NewEvent {
        NewEvent {
            kind: self.kind.clone(),
            item: self.item.clone(),
            from_node: self.from_node.clone(),
            to_node: self.to_node.clone(),
            repo: self.repo.clone(),
            lane: self.lane.clone(),
            sha: self.sha.clone(),
            body: self.body.clone(),
            packet: self.packet.clone(),
            key: Some(self.key.clone()),
            root_event: self.root_event,
            answer: self.answer,
        }
    }
}

pub(crate) struct EventStore {
    conn: Connection,
}

fn sql(error: rusqlite::Error) -> NorthError {
    NorthError::State(format!("event store: {error}"))
}

fn refuse(message: &str) -> NorthError {
    NorthError::Usage(message.into())
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

fn select(tx: &Connection, filter: &str, value: &dyn rusqlite::ToSql) -> NorthResult<Option<Event>> {
    tx.query_row(&format!("SELECT {COLUMNS} FROM agent_events WHERE {filter}=?1"), [value], Event::from_row)
        .optional()
        .map_err(sql)
}

fn addressed(tx: &Connection, id: i64) -> NorthResult<Event> {
    select(tx, "id", &id)?.ok_or_else(|| refuse(&format!("no addressed event {id}")))
}

fn new_key(tx: &Connection) -> NorthResult<String> {
    let mut bytes: Vec<u8> = tx.query_row("SELECT randomblob(16)", [], |row| row.get(0)).map_err(sql)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]))
}

fn answer_targets(tx: &Connection, event: &NewEvent, answer: i64) -> NorthResult<Vec<Event>> {
    let target = addressed(tx, answer)?;
    if target.kind == "ANSWER" || target.answered_by.is_some() {
        return Err(refuse("event already answered or is an ANSWER"));
    }
    if event.from_node != target.to_node && event.from_node != "proxy" {
        return Err(refuse("only the recipient or proxy may answer"));
    }
    if event.to_node != target.from_node {
        return Err(refuse("answer recipient must be the event sender"));
    }
    let mut targets = vec![target];
    if let Some(linked) = targets[0].root_event {
        targets.push(addressed(tx, linked)?);
    }
    if let Some(root) = event.root_event
        && !targets.iter().any(|row| row.id == root)
    {
        return Err(refuse("answer root must match the referenced event root"));
    }
    targets.retain(|row| row.answered_by.is_none());
    Ok(targets)
}

impl EventStore {
    pub(crate) fn default_path() -> NorthResult<PathBuf> {
        let home = std::env::var_os("HOME").ok_or_else(|| NorthError::Configuration("No home directory for the event store".into()))?;
        Ok(PathBuf::from(home).join(".local/state/north/north.db"))
    }

    pub(crate) fn open(path: &Path) -> NorthResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(NorthError::Io)?;
        }
        let conn = Connection::open(path).map_err(sql)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(sql)?;
        conn.execute_batch(SCHEMA).map_err(sql)?;
        Ok(Self { conn })
    }

    pub(crate) fn append(&mut self, event: &NewEvent, timestamp: &str) -> NorthResult<Event> {
        if !KINDS.contains(&event.kind.as_str()) || blank(&event.from_node) || blank(&event.to_node) {
            return Err(refuse("agent-event requires a supervisor kind and addressed sender/receiver"));
        }
        let mut event = event.clone();
        if event.key.is_none() {
            event.key = Some(new_key(&self.conn)?);
        }
        let key = event.key.clone().unwrap_or_default();
        if blank(&key) {
            return Err(refuse("key must be nonempty text"));
        }
        if (event.kind == "ANSWER") != event.answer.is_some() {
            return Err(refuse("ANSWER requires --answer; other kinds cannot use --answer"));
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        if let Some(existing) = select(&tx, "key", &key)? {
            if existing.payload() != event {
                return Err(refuse("key already names a different payload"));
            }
            return Ok(existing);
        }
        if let Some(root) = event.root_event
            && addressed(&tx, root)?.kind == "ANSWER"
        {
            return Err(refuse("root event cannot be an ANSWER"));
        }
        let targets = match event.answer {
            Some(answer) => answer_targets(&tx, &event, answer)?,
            None => Vec::new(),
        };
        tx.execute(
            "INSERT INTO agent_events(time,item,kind,from_node,to_node,repo,lane,sha,body,packet,key,root_event,answer) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![timestamp, event.item, event.kind, event.from_node, event.to_node, event.repo, event.lane, event.sha, event.body, event.packet, key, event.root_event, event.answer],
        )
        .map_err(sql)?;
        let id = tx.last_insert_rowid();
        for target in &targets {
            tx.execute("UPDATE agent_events SET answered_by=?1 WHERE id=?2", params![id, target.id]).map_err(sql)?;
        }
        let stored = addressed(&tx, id)?;
        tx.commit().map_err(sql)?;
        Ok(stored)
    }
}

#[cfg(test)]
mod append_tests {
    use super::*;

    fn store() -> (tempfile::TempDir, EventStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = EventStore::open(&dir.path().join("state/north.db")).unwrap();
        (dir, store)
    }

    fn event(kind: &str, from: &str, to: &str, body: &str) -> NewEvent {
        NewEvent {
            kind: kind.into(),
            from_node: from.into(),
            to_node: to.into(),
            body: Some(body.into()),
            ..NewEvent::default()
        }
    }

    fn answer(from: &str, to: &str, body: &str, target: i64) -> NewEvent {
        NewEvent { answer: Some(target), ..event("ANSWER", from, to, body) }
    }

    fn rooted(mut event: NewEvent, root: i64) -> NewEvent {
        event.root_event = Some(root);
        event
    }

    fn keyed(mut event: NewEvent, key: &str) -> NewEvent {
        event.key = Some(key.into());
        event
    }

    fn get(store: &EventStore, id: i64) -> Event {
        addressed(&store.conn, id).unwrap()
    }

    fn refusal(result: NorthResult<Event>) -> String {
        match result {
            Err(NorthError::Usage(message)) => message,
            other => panic!("expected refusal, got {other:?}"),
        }
    }

    fn count(store: &EventStore) -> i64 {
        store.conn.query_row("SELECT count(*) FROM agent_events", [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn opens_in_wal_mode() {
        let (_dir, store) = store();
        let mode: String = store.conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn same_key_and_payload_returns_the_original_row() {
        let (_dir, mut store) = store();
        let stuck = NewEvent { item: Some("ops#1".into()), ..keyed(event("STUCK", "worker", "lead", "blocked"), "seed") };
        let first = store.append(&stuck, "t1").unwrap();
        let replay = store.append(&stuck, "t2").unwrap();
        assert_eq!(first, replay);
        assert_eq!(count(&store), 1);
    }

    #[test]
    fn same_key_with_a_different_payload_is_refused() {
        let (_dir, mut store) = store();
        store.append(&NewEvent { item: Some("ops#1".into()), ..keyed(event("STUCK", "worker", "lead", "blocked"), "seed") }, "t1").unwrap();
        let changed = refusal(store.append(&keyed(event("STUCK", "worker", "lead", "changed"), "seed"), "t2"));
        assert_eq!(changed, "key already names a different payload");
        assert_eq!(count(&store), 1);
    }

    #[test]
    fn answer_requires_an_answer_target() {
        let (_dir, mut store) = store();
        let missing = refusal(store.append(&event("ANSWER", "lead", "worker", "no target"), "t1"));
        assert_eq!(missing, "ANSWER requires --answer; other kinds cannot use --answer");
        let stuck = store.append(&event("STUCK", "worker", "lead", "blocked"), "t1").unwrap();
        let stray = refusal(store.append(&NewEvent { answer: Some(stuck.id), ..event("QUESTION", "lead", "worker", "stray") }, "t2"));
        assert_eq!(stray, "ANSWER requires --answer; other kinds cannot use --answer");
    }

    #[test]
    fn answer_sets_answered_by_on_the_target_and_its_root() {
        let (_dir, mut store) = store();
        let stuck = store.append(&event("STUCK", "worker", "lead", "blocked"), "t1").unwrap();
        let other = store.append(&event("QUESTION", "worker", "lead", "unanswered"), "t2").unwrap();
        let linked = store.append(&rooted(event("QUESTION", "worker", "proxy", "escalate"), stuck.id), "t3").unwrap();
        let reply = store.append(&keyed(answer("proxy", "worker", "yes", linked.id), "answer"), "t4").unwrap();
        assert_eq!(get(&store, linked.id).answered_by, Some(reply.id));
        assert_eq!(get(&store, stuck.id).answered_by, Some(reply.id));
        assert_eq!(get(&store, other.id).answered_by, None);
        let replay = store.append(&keyed(answer("proxy", "worker", "yes", linked.id), "answer"), "t5").unwrap();
        assert_eq!(replay.id, reply.id);
        let root = store.append(&event("STUCK", "worker", "lead", "delegated"), "t6").unwrap();
        let forwarded = store.append(&rooted(event("QUESTION", "worker", "specialist", "delegate"), root.id), "t7").unwrap();
        let original = store.append(&answer("lead", "worker", "root solved", root.id), "t8").unwrap();
        let last = store.append(&answer("specialist", "worker", "forward solved", forwarded.id), "t9").unwrap();
        assert_eq!(get(&store, root.id).answered_by, Some(original.id));
        assert_eq!(get(&store, forwarded.id).answered_by, Some(last.id));
        let delegated = store.append(&event("QUESTION", "worker", "lead", "new delegated"), "t10").unwrap();
        let child = store.append(&rooted(event("QUESTION", "worker", "specialist", "delegate"), delegated.id), "t11").unwrap();
        let resolved = store.append(&answer("specialist", "worker", "solved", child.id), "t12").unwrap();
        assert_eq!(get(&store, delegated.id).answered_by, Some(resolved.id));
    }

    #[test]
    fn only_the_recipient_or_proxy_may_answer() {
        let (_dir, mut store) = store();
        let stuck = store.append(&event("STUCK", "worker", "lead", "blocked"), "t1").unwrap();
        let linked = store.append(&rooted(event("QUESTION", "worker", "proxy", "escalate"), stuck.id), "t2").unwrap();
        let intruder = refusal(store.append(&answer("intruder", "worker", "no", linked.id), "t3"));
        assert_eq!(intruder, "only the recipient or proxy may answer");
        let root = store.append(&event("STUCK", "worker", "lead", "delegated"), "t4").unwrap();
        let forwarded = store.append(&rooted(event("QUESTION", "worker", "specialist", "delegate"), root.id), "t5").unwrap();
        let wrong = refusal(store.append(&answer("lead", "worker", "wrong exact recipient", forwarded.id), "t6"));
        assert_eq!(wrong, "only the recipient or proxy may answer");
        let destination = refusal(store.append(&answer("specialist", "stranger", "wrong destination", forwarded.id), "t7"));
        assert_eq!(destination, "answer recipient must be the event sender");
        assert_eq!(get(&store, forwarded.id).answered_by, None);
    }

    #[test]
    fn an_already_answered_target_is_refused() {
        let (_dir, mut store) = store();
        let stuck = store.append(&event("STUCK", "worker", "lead", "blocked"), "t1").unwrap();
        let reply = store.append(&answer("lead", "worker", "done", stuck.id), "t2").unwrap();
        let again = refusal(store.append(&answer("lead", "worker", "again", stuck.id), "t3"));
        assert_eq!(again, "event already answered or is an ANSWER");
        let nested = refusal(store.append(&answer("worker", "lead", "nested", reply.id), "t4"));
        assert_eq!(nested, "event already answered or is an ANSWER");
    }

    #[test]
    fn the_root_cannot_be_an_answer() {
        let (_dir, mut store) = store();
        let stuck = store.append(&event("STUCK", "worker", "lead", "blocked"), "t1").unwrap();
        let reply = store.append(&answer("lead", "worker", "done", stuck.id), "t2").unwrap();
        let rootless = refusal(store.append(&rooted(event("QUESTION", "worker", "lead", "follow up"), reply.id), "t3"));
        assert_eq!(rootless, "root event cannot be an ANSWER");
        assert_eq!(count(&store), 2);
    }
}
