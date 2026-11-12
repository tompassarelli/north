//! The sweep that makes a deadline act: an instance past its state's dwell moves to the state its machine's
//! [`Lifecycle::expire`] names, and the move queues its effect in the same transaction (north#1235).
use rusqlite::{TransactionBehavior, params};

use super::{NorthResult, Store, Timestamp, log, outbox, read, sql, typed, write};
use crate::lifecycle::{Effect, Lifecycle, Record, State};

/// The most instances of one machine one sweep moves, earliest deadline first, so a backlog drains over several
/// sweeps rather than holding the store's write lock for one long one.
pub const EXPIRE_BOUND: usize = 100;

/// The cause every expiry move is logged with ends in this.
pub const EXPIRED: &str = "deadline passed";

impl Store {
    /// Moves up to [`EXPIRE_BOUND`] instances of `L` whose deadline passed before `at` to the state `L::expire` names
    /// for their state, each in its own transaction with the effect that move queues, and returns the moved records.
    /// Each move rereads its record under the write lock and moves only the stay it found overdue, and the effect is
    /// keyed by that stay, so sweeps on two hosts at once move the instance and queue its effect once.
    pub fn expire<L: Lifecycle>(&mut self, at: Timestamp) -> NorthResult<Vec<Record<L::State>>> {
        let expiring: Vec<&str> = L::State::ALL.iter().filter(|state| L::expire(**state).is_some()).map(|state| state.name()).collect();
        if expiring.is_empty() {
            return Ok(Vec::new());
        }
        let overdue: Vec<(String, i64)> = {
            let mut statement = self
                .connection
                .prepare("SELECT instance, seq FROM lifecycle_states WHERE machine = ?1 AND due_ms < ?2 AND state IN (SELECT value FROM json_each(?3)) ORDER BY due_ms LIMIT ?4")
                .map_err(sql)?;
            let rows = statement
                .query_map(params![L::NAME, at.unix_millis(), serde_json::to_string(&expiring)?, i64::try_from(EXPIRE_BOUND).unwrap_or(i64::MAX)], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(sql)?;
            rows.collect::<Result<_, _>>().map_err(sql)?
        };
        let mut moved = Vec::new();
        for (instance, seq) in overdue {
            moved.extend(self.expiring::<L>(&instance, seq, at)?);
        }
        Ok(moved)
    }

    fn expiring<L: Lifecycle>(&mut self, instance: &str, seq: i64, at: Timestamp) -> NorthResult<Option<Record<L::State>>> {
        let transaction = self.connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        let Some(record) = read(&transaction, L::NAME, instance)?.map(|row| typed::<L>(instance, row)).transpose()? else { return Ok(None) };
        let stay = record.seq == seq && record.due.is_some_and(|due| due < at);
        let Some(expiry) = L::expire(record.state).filter(|_| stay) else { return Ok(None) };
        let cause = format!("its {} {EXPIRED}", record.state.name());
        if !L::legal(Some(record.state), expiry.to) {
            log(&transaction, L::NAME, instance, Some(record.state.name()), expiry.to.name(), at, &self.who, &cause, true)?;
            transaction.commit().map_err(sql)?;
            return Ok(None);
        }
        let effect = Effect {
            kind: expiry.kind.to_owned(),
            key: format!("{}/{instance}/{}/expired", L::NAME, record.seq),
            payload: serde_json::json!({
                "machine": L::NAME,
                "instance": instance,
                "from": record.state.name(),
                "to": expiry.to.name(),
                "due_ms": record.due.map(Timestamp::unix_millis),
                "cause": record.cause,
            }),
            delivery: expiry.delivery,
        };
        let moved = write::<L>(&transaction, (&self.who, &self.who), &record, expiry.to, &cause, at, None)?;
        outbox::queue(&transaction, L::NAME, instance, &self.who, &[effect], at)?;
        transaction.commit().map_err(sql)?;
        Ok(Some(moved))
    }
}
