use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::{Fault, FaultKind, Lifecycle, Liveness, Move, Moved, Owner, Proc, Record, Refusal, State};
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS lifecycle_states(
  machine TEXT NOT NULL CHECK(length(machine) > 0),
  instance TEXT NOT NULL,
  state TEXT NOT NULL CHECK(length(state) > 0),
  since_ms INTEGER NOT NULL,
  due_ms INTEGER,
  owner TEXT CHECK(owner IS NULL OR json_valid(owner)),
  cause TEXT NOT NULL,
  seq INTEGER NOT NULL CHECK(seq > 0),
  PRIMARY KEY(machine, instance)
) STRICT;
CREATE TABLE IF NOT EXISTS lifecycle_moves(
  move_id INTEGER PRIMARY KEY,
  machine TEXT NOT NULL,
  instance TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  at_ms INTEGER NOT NULL,
  moved_by TEXT NOT NULL CHECK(json_valid(moved_by)),
  cause TEXT NOT NULL,
  refused INTEGER NOT NULL CHECK(refused IN (0, 1))
) STRICT;
CREATE INDEX IF NOT EXISTS lifecycle_moves_by_instance ON lifecycle_moves(machine, instance, at_ms);
";

/// Logged moves older than this are pruned at the instance's next move.
const KEEP_MOVES: Duration = Duration::from_secs(30 * 24 * 3600);
/// A refused illegal move stays a fault this long.
const ILLEGAL_WINDOW: Duration = Duration::from_secs(3600);

/// The records of every instance of every machine kept in one SQLite file, and the log of their moves.
pub struct Store {
    connection: Connection,
    who: Owner,
    liveness: Box<dyn Liveness>,
}

fn sql(error: rusqlite::Error) -> NorthError {
    NorthError::State(format!("lifecycle store: {error}"))
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

struct Row {
    state: String,
    since_ms: i64,
    due_ms: Option<i64>,
    owner: Option<Owner>,
    cause: String,
    seq: i64,
}

impl Store {
    /// This host's store, beside the run ledger in `~/.local/state/north/ledger.db`.
    pub fn host() -> NorthResult<Self> {
        Self::open(&Self::host_path()?)
    }

    pub fn host_path() -> NorthResult<PathBuf> {
        let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or_else(|| NorthError::Configuration("HOME is not set".into()))?;
        Ok(PathBuf::from(home).join(".local/state/north/ledger.db"))
    }

    pub fn open(path: &Path) -> NorthResult<Self> {
        Self::open_as(path, Owner::current(), Box::new(Proc))
    }

    /// A store whose moves are made by `who`, judging holders with `liveness`.
    pub fn open_as(path: &Path, who: Owner, liveness: Box<dyn Liveness>) -> NorthResult<Self> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path).map_err(sql)?;
        connection.busy_timeout(Duration::from_secs(10)).map_err(sql)?;
        connection.execute_batch(SCHEMA).map_err(sql)?;
        Ok(Self { connection, who, liveness })
    }

    pub fn current<L: Lifecycle>(&self, instance: &str) -> NorthResult<Option<Record<L::State>>> {
        read(&self.connection, L::NAME, instance)?.map(|row| typed::<L>(instance, row)).transpose()
    }

    /// Every instance of `L`, oldest record first.
    pub fn instances<L: Lifecycle>(&self) -> NorthResult<Vec<Record<L::State>>> {
        let mut statement = self.connection.prepare("SELECT instance FROM lifecycle_states WHERE machine = ?1 ORDER BY since_ms").map_err(sql)?;
        let names: Vec<String> = statement.query_map([L::NAME], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
        names.iter().filter_map(|name| self.current::<L>(name).transpose()).collect()
    }

    /// Every instance of `L` whose name starts with `prefix`, oldest record first.
    pub fn instances_under<L: Lifecycle>(&self, prefix: &str) -> NorthResult<Vec<Record<L::State>>> {
        let mut statement = self
            .connection
            .prepare("SELECT instance, state, since_ms, due_ms, owner, cause, seq FROM lifecycle_states WHERE machine = ?1 AND substr(instance, 1, length(?2)) = ?2 ORDER BY since_ms")
            .map_err(sql)?;
        let rows = statement
            .query_map([L::NAME, prefix], |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get::<_, Option<String>>(4)?, row.get(5)?, row.get(6)?)))
            .map_err(sql)?;
        rows.map(|row| {
            let (instance, state, since_ms, due_ms, owner, cause, seq) = row.map_err(sql)?;
            typed::<L>(&instance, Row { state, since_ms, due_ms, owner: owner.map(|owner| serde_json::from_str(&owner)).transpose()?, cause, seq })
        })
        .collect()
    }

    /// Deletes the instances of `L` that no process holds and that have not moved since `before`, with their logged moves; returns how many.
    pub fn forget<L: Lifecycle>(&mut self, before: Timestamp) -> NorthResult<usize> {
        let transaction = self.connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        let resting = "SELECT instance FROM lifecycle_states WHERE machine = ?1 AND owner IS NULL AND since_ms < ?2";
        transaction.execute(&format!("DELETE FROM lifecycle_moves WHERE machine = ?1 AND instance IN ({resting})"), params![L::NAME, before.unix_millis()]).map_err(sql)?;
        let forgotten = transaction.execute("DELETE FROM lifecycle_states WHERE machine = ?1 AND owner IS NULL AND since_ms < ?2", params![L::NAME, before.unix_millis()]).map_err(sql)?;
        transaction.commit().map_err(sql)?;
        Ok(forgotten)
    }

    /// Moves `instance` to `to` for `cause`, the only way its record changes. A state held by a process that is
    /// gone first moves to the machine's orphan state, logged with the holder, so the log says what happened.
    pub fn transition<L: Lifecycle>(&mut self, instance: &str, to: L::State, cause: &str, at: Timestamp) -> NorthResult<Moved<L::State>> {
        self.transition_by::<L>(instance, to, cause, at, None)
    }

    /// [`Self::transition`] for a state whose limit belongs to the instance, as a gate's own default deadline does, so
    /// `deadline` replaces the machine's dwell for this stay.
    pub fn transition_by<L: Lifecycle>(&mut self, instance: &str, to: L::State, cause: &str, at: Timestamp, deadline: Option<Timestamp>) -> NorthResult<Moved<L::State>> {
        let holder = self.who.clone();
        self.moving::<L>(instance, to, cause, at, &holder, deadline)
    }

    /// As [`Store::transition`], with `holder` rather than the mover holding a state a process holds: a launcher
    /// records the process it started.
    pub fn transition_for<L: Lifecycle>(&mut self, instance: &str, to: L::State, cause: &str, at: Timestamp, holder: &Owner) -> NorthResult<Moved<L::State>> {
        self.moving::<L>(instance, to, cause, at, holder, None)
    }

    /// The record of `instance`, first moved to its orphan state when the process holding it is gone, so a reader
    /// never takes a dead holder's state for a live one.
    pub fn settle<L: Lifecycle>(&mut self, instance: &str, at: Timestamp) -> NorthResult<Option<Record<L::State>>> {
        let current = self.current::<L>(instance)?;
        let dead = current.as_ref().and_then(|record| record.owner.as_ref().filter(|owner| **owner != self.who)).is_some_and(|owner| self.liveness.alive(owner) == Some(false));
        if !dead {
            return Ok(current);
        }
        let transaction = self.connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        let current = read(&transaction, L::NAME, instance)?.map(|row| typed::<L>(instance, row)).transpose()?;
        let settled = release::<L>(&transaction, &self.who, self.liveness.as_ref(), current, None, at)?;
        transaction.commit().map_err(sql)?;
        match settled {
            Ok(record) => Ok(record),
            Err(_) => self.current::<L>(instance),
        }
    }

    fn moving<L: Lifecycle>(&mut self, instance: &str, to: L::State, cause: &str, at: Timestamp, holder: &Owner, deadline: Option<Timestamp>) -> NorthResult<Moved<L::State>> {
        let transaction = self.connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        let current = read(&transaction, L::NAME, instance)?.map(|row| typed::<L>(instance, row)).transpose()?;
        let current = match release::<L>(&transaction, &self.who, self.liveness.as_ref(), current, Some(to), at)? {
            Ok(current) => current,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if !L::legal(current.as_ref().map(|record| record.state), to) {
            log(&transaction, L::NAME, instance, current.as_ref().map(|record| record.state.name()), to.name(), at, &self.who, cause, true)?;
            transaction.commit().map_err(sql)?;
            return Ok(Err(Refusal::Illegal { from: current.map(|record| record.state), to }));
        }
        let first = Record { instance: instance.to_owned(), state: to, since: at, due: None, owner: None, cause: String::new(), seq: 0 };
        let record = write::<L>(&transaction, (&self.who, holder), current.as_ref().unwrap_or(&first), to, cause, at, deadline)?;
        prune::<L>(&transaction, at)?;
        transaction.commit().map_err(sql)?;
        Ok(Ok(record))
    }

    /// The logged moves of one instance, oldest first.
    pub fn history(&self, machine: &str, instance: &str) -> NorthResult<Vec<Move>> {
        let mut statement = self
            .connection
            .prepare("SELECT from_state, to_state, at_ms, moved_by, cause, refused FROM lifecycle_moves WHERE machine = ?1 AND instance = ?2 ORDER BY move_id")
            .map_err(sql)?;
        let rows = statement
            .query_map([machine, instance], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, String>(3)?, row.get(4)?, row.get::<_, i64>(5)?)))
            .map_err(sql)?;
        rows.map(|row| {
            let (from, to, at_ms, by, cause, refused) = row.map_err(sql)?;
            Ok(Move { from, to, at: Timestamp::from_unix_millis(at_ms), by: serde_json::from_str(&by)?, cause, refused: refused == 1 })
        })
        .collect()
    }

    /// Every stuck instance and every illegal move of the last hour, of every machine, read without their types.
    pub fn faults(&self, at: Timestamp) -> NorthResult<Vec<Fault>> {
        let mut faults = Vec::new();
        let mut statement = self
            .connection
            .prepare("SELECT machine, instance, state, since_ms, due_ms, owner, cause FROM lifecycle_states WHERE due_ms IS NOT NULL OR owner IS NOT NULL ORDER BY since_ms")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get::<_, Option<i64>>(4)?, row.get::<_, Option<String>>(5)?, row.get(6)?)))
            .map_err(sql)?;
        for row in rows {
            let (machine, instance, state, since_ms, due_ms, owner, cause) = row.map_err(sql)?;
            let owner: Option<Owner> = owner.map(|owner| serde_json::from_str(&owner)).transpose()?;
            let kind = match (owner, due_ms) {
                (Some(owner), _) if self.liveness.alive(&owner) == Some(false) => FaultKind::Orphaned { owner },
                (_, Some(due_ms)) if due_ms < at.unix_millis() => FaultKind::Overdue { due_ms },
                _ => continue,
            };
            faults.push(Fault { machine, instance, state, since_ms, kind, cause });
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT m.machine, m.instance, coalesce(s.state, m.from_state, ''), coalesce(s.since_ms, m.at_ms), m.to_state, m.at_ms, m.cause FROM lifecycle_moves m \
                 LEFT JOIN lifecycle_states s ON s.machine = m.machine AND s.instance = m.instance WHERE m.refused = 1 AND m.at_ms >= ?1 ORDER BY m.move_id",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map([at.unix_millis() - millis(ILLEGAL_WINDOW)], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)))
            .map_err(sql)?;
        for row in rows {
            let (machine, instance, state, since_ms, to, at_ms, cause) = row.map_err(sql)?;
            faults.push(Fault { machine, instance, state, since_ms, kind: FaultKind::Illegal { to, at_ms }, cause });
        }
        Ok(faults)
    }
}

fn read(connection: &Connection, machine: &str, instance: &str) -> NorthResult<Option<Row>> {
    connection
        .query_row(
            "SELECT state, since_ms, due_ms, owner, cause, seq FROM lifecycle_states WHERE machine = ?1 AND instance = ?2",
            [machine, instance],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, Option<String>>(3)?, row.get(4)?, row.get(5)?)),
        )
        .optional()
        .map_err(sql)?
        .map(|(state, since_ms, due_ms, owner, cause, seq)| Ok(Row { state, since_ms, due_ms, owner: owner.map(|owner| serde_json::from_str(&owner)).transpose()?, cause, seq }))
        .transpose()
}

fn typed<L: Lifecycle>(instance: &str, row: Row) -> NorthResult<Record<L::State>> {
    let state = L::State::parse(&row.state).ok_or_else(|| NorthError::State(format!("{} {instance} is in {}, a state the machine does not have", L::NAME, row.state)))?;
    Ok(Record {
        instance: instance.to_owned(),
        state,
        since: Timestamp::from_unix_millis(row.since_ms),
        due: row.due_ms.map(Timestamp::from_unix_millis),
        owner: row.owner,
        cause: row.cause,
        seq: row.seq,
    })
}

/// The record a move may proceed from, or why a live holder keeps it.
type Released<S> = Result<Option<Record<S>>, Refusal<S>>;

/// Moves a state held by another process out of the way of a move to `to` (`None` when only settling): a live
/// holder refuses it unless the machine lets `to` preempt the state, and a dead one moves it to the orphan state.
fn release<L: Lifecycle>(transaction: &Transaction, who: &Owner, liveness: &dyn Liveness, current: Option<Record<L::State>>, to: Option<L::State>, at: Timestamp) -> NorthResult<Released<L::State>> {
    let Some(record) = current else { return Ok(Ok(None)) };
    let Some(owner) = record.owner.clone().filter(|owner| owner != who) else { return Ok(Ok(Some(record))) };
    let overdue = record.due.is_some_and(|due| at > due);
    if liveness.alive(&owner).unwrap_or(!overdue) {
        return Ok(if to.is_some_and(|to| L::preempts(record.state, to)) { Ok(Some(record)) } else { Err(Refusal::Held { state: record.state, owner }) });
    }
    let orphan = L::orphan(record.state).ok_or_else(|| NorthError::State(format!("{} {} is held in {}, which has no orphan state", L::NAME, record.instance, record.state.name())))?;
    Ok(Ok(Some(write::<L>(transaction, (who, who), &record, orphan, &format!("the holder, {owner}, is gone"), at, None)?)))
}

fn write<L: Lifecycle>(transaction: &Transaction, (who, holder): (&Owner, &Owner), from: &Record<L::State>, to: L::State, cause: &str, at: Timestamp, deadline: Option<Timestamp>) -> NorthResult<Record<L::State>> {
    let previous = (from.seq > 0).then(|| from.state.name());
    let record = Record {
        instance: from.instance.clone(),
        state: to,
        since: at,
        due: deadline.or_else(|| L::dwell(to).map(|dwell| Timestamp::from_unix_millis(at.unix_millis().saturating_add(millis(dwell))))),
        owner: L::orphan(to).map(|_| holder.clone()),
        cause: cause.to_owned(),
        seq: from.seq + 1,
    };
    let owner = record.owner.as_ref().map(serde_json::to_string).transpose()?;
    transaction
        .execute(
            "INSERT INTO lifecycle_states(machine, instance, state, since_ms, due_ms, owner, cause, seq) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(machine, instance) DO UPDATE SET state = excluded.state, since_ms = excluded.since_ms, due_ms = excluded.due_ms, owner = excluded.owner, cause = excluded.cause, seq = excluded.seq",
            params![L::NAME, record.instance, to.name(), at.unix_millis(), record.due.map(Timestamp::unix_millis), owner, cause, record.seq],
        )
        .map_err(sql)?;
    log(transaction, L::NAME, &record.instance, previous, to.name(), at, who, cause, false)?;
    Ok(record)
}

#[allow(clippy::too_many_arguments, reason = "one logged move is these eight columns")]
fn log(transaction: &Transaction, machine: &str, instance: &str, from: Option<&str>, to: &str, at: Timestamp, who: &Owner, cause: &str, refused: bool) -> NorthResult<()> {
    transaction
        .execute(
            "INSERT INTO lifecycle_moves(machine, instance, from_state, to_state, at_ms, moved_by, cause, refused) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![machine, instance, from, to, at.unix_millis(), serde_json::to_string(who)?, cause, i64::from(refused)],
        )
        .map_err(sql)?;
    Ok(())
}

/// Drops the machine's records that rested past their state's keep, and its logged moves older than [`KEEP_MOVES`].
fn prune<L: Lifecycle>(transaction: &Transaction, at: Timestamp) -> NorthResult<()> {
    for state in L::State::ALL.iter().copied() {
        if let Some(keep) = L::keep(state) {
            transaction
                .execute("DELETE FROM lifecycle_states WHERE machine = ?1 AND state = ?2 AND since_ms < ?3", params![L::NAME, state.name(), at.unix_millis() - millis(keep)])
                .map_err(sql)?;
        }
    }
    transaction.execute("DELETE FROM lifecycle_moves WHERE machine = ?1 AND at_ms < ?2", params![L::NAME, at.unix_millis() - millis(KEEP_MOVES)]).map_err(sql)?;
    Ok(())
}
