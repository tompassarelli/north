//! The side effects a move starts, committed in the move's transaction and delivered from one table, so a crash
//! between the move and its effect neither loses nor repeats the effect (north#862).
use std::time::Duration;

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::{Store, millis, sql};
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::lifecycle::{Delivery, Effect, Leased, Lifecycle, Liveness, Moved, Owner, Stage};

/// Leases an effect gets before it stays pending for `north health` to report.
pub(crate) const MAX_ATTEMPTS: i64 = 5;
/// How long a failed delivery waits before its next lease.
const RETRY: Duration = Duration::from_secs(60);

impl Delivery {
    fn name(self) -> &'static str {
        match self {
            Self::ReplaySafe => "replay-safe",
            Self::ProcessBound => "process-bound",
        }
    }

    fn parse(name: &str) -> NorthResult<Self> {
        [Self::ReplaySafe, Self::ProcessBound].into_iter().find(|delivery| delivery.name() == name).ok_or_else(|| NorthError::State(format!("lifecycle store: an effect has no delivery {name}")))
    }
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Leased => "leased",
            Self::Delivered => "delivered",
            Self::Retired => "retired",
        }
    }

    fn parse(name: &str) -> NorthResult<Self> {
        [Self::Pending, Self::Leased, Self::Delivered, Self::Retired].into_iter().find(|stage| stage.name() == name).ok_or_else(|| NorthError::State(format!("lifecycle store: an effect has no stage {name}")))
    }
}

impl Store {
    /// [`Store::transition`] that also queues `effects` in the move's transaction, so the move and the effects it
    /// starts commit together or not at all; a refused move queues none. A `holder` holds a state a process holds in
    /// the mover's place, as with [`Store::transition_for`].
    pub fn transition_with_effects<L: Lifecycle>(&mut self, instance: &str, to: L::State, cause: &str, at: Timestamp, holder: Option<&Owner>, effects: &[Effect]) -> NorthResult<Moved<L::State>> {
        let holder = holder.cloned().unwrap_or_else(|| self.who.clone());
        self.moving::<L>(instance, to, cause, at, (&holder, None), effects)
    }

    /// The oldest pending effect of `kind` this process may deliver, leased to it for `hold`. First requeues every
    /// lease of `kind` whose lessee is gone or whose hold ran out, and retires every process-bound effect of `kind`
    /// whose committing process is gone.
    pub fn lease(&mut self, kind: &str, at: Timestamp, hold: Duration) -> NorthResult<Option<Leased>> {
        let transaction = self.connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        recover(&transaction, (&self.who, self.liveness.as_ref()), kind, at)?;
        let who = serde_json::to_string(&self.who)?;
        let next = transaction
            .query_row(
                "SELECT effect_id, machine, instance, key, payload, delivery, attempts, created_ms FROM lifecycle_effects WHERE kind = ?1 AND stage = 'pending' AND attempts < ?2 AND ready_ms <= ?3 \
                 AND (delivery = 'replay-safe' OR owner = ?4) ORDER BY effect_id LIMIT 1",
                params![kind, MAX_ATTEMPTS, at.unix_millis(), who],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((id, machine, instance, key, payload, delivery, attempts, created_ms)) = next else {
            transaction.commit().map_err(sql)?;
            return Ok(None);
        };
        transaction
            .execute(
                "UPDATE lifecycle_effects SET stage = 'leased', attempts = attempts + 1, lessee = ?2, lease_until_ms = ?3 WHERE effect_id = ?1",
                params![id, who, at.unix_millis().saturating_add(millis(hold))],
            )
            .map_err(sql)?;
        transaction.commit().map_err(sql)?;
        let effect = Effect { kind: kind.to_owned(), key, payload: serde_json::from_str(&payload)?, delivery: Delivery::parse(&delivery)? };
        Ok(Some(Leased { machine, instance, effect, attempts: attempts + 1, queued: Timestamp::from_unix_millis(created_ms) }))
    }

    /// Records the effect under `key`, leased to this process, delivered; false when its lease was lost to another
    /// process first.
    pub fn delivered(&mut self, key: &str, at: Timestamp) -> NorthResult<bool> {
        self.settle_lease(key, "stage = 'delivered', settled_ms = ?3, note = ?4, lessee = NULL, lease_until_ms = NULL", at, "")
    }

    /// Hands the effect under `key`, leased to this process, back for a later lease after its delivery failed with
    /// `error`.
    pub fn undelivered(&mut self, key: &str, error: &str, at: Timestamp) -> NorthResult<bool> {
        let ready = format!("stage = 'pending', ready_ms = ?3 + {}, note = ?4, lessee = NULL, lease_until_ms = NULL", millis(RETRY));
        self.settle_lease(key, &ready, at, error)
    }

    /// Hands the effect under `key`, leased to this process, back undelivered and uncounted, as a process that stops
    /// cleanly does with the effects it still holds; false when it holds no lease on it.
    pub fn release(&mut self, key: &str, at: Timestamp) -> NorthResult<bool> {
        self.settle_lease(key, "stage = 'pending', attempts = attempts - 1, note = ?4, lessee = NULL, lease_until_ms = NULL, ready_ms = min(ready_ms, ?3)", at, "handed back by its lessee")
    }

    /// Retires the open effect under `key` for `reason`, as a process does with an effect it no longer means to
    /// deliver; false when no open effect has that key.
    pub fn retire(&mut self, key: &str, reason: &str, at: Timestamp) -> NorthResult<bool> {
        let retired = self
            .connection
            .execute(
                "UPDATE lifecycle_effects SET stage = 'retired', settled_ms = ?2, note = ?3, lessee = NULL, lease_until_ms = NULL WHERE key = ?1 AND stage IN ('pending', 'leased')",
                params![key, at.unix_millis(), reason],
            )
            .map_err(sql)?;
        Ok(retired == 1)
    }

    /// Leases and sends every effect of `kind` this process may deliver, one at a time, recording each delivered or
    /// handed back with `send`'s error; returns how many were delivered.
    pub fn deliver(&mut self, kind: &str, at: Timestamp, hold: Duration, mut send: impl FnMut(&Leased) -> Result<(), String>) -> NorthResult<usize> {
        let mut delivered = 0;
        while let Some(leased) = self.lease(kind, at, hold)? {
            match send(&leased) {
                Ok(()) => delivered += usize::from(self.delivered(&leased.effect.key, at)?),
                Err(error) => {
                    self.undelivered(&leased.effect.key, &error, at)?;
                }
            }
        }
        Ok(delivered)
    }

    /// The instances with an effect of `kind` not yet delivered or retired, oldest effect first.
    pub fn open_effects(&self, kind: &str) -> NorthResult<Vec<String>> {
        let mut statement = self.connection.prepare("SELECT instance FROM lifecycle_effects WHERE kind = ?1 AND stage IN ('pending', 'leased') ORDER BY effect_id").map_err(sql)?;
        statement.query_map([kind], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)
    }

    /// Where the effect under `key` is, `None` when none was queued or it was pruned.
    pub fn stage(&self, key: &str) -> NorthResult<Option<Stage>> {
        let stage: Option<String> = self.connection.query_row("SELECT stage FROM lifecycle_effects WHERE key = ?1", [key], |row| row.get(0)).optional().map_err(sql)?;
        stage.as_deref().map(Stage::parse).transpose()
    }

    fn settle_lease(&mut self, key: &str, set: &str, at: Timestamp, note: &str) -> NorthResult<bool> {
        let who = serde_json::to_string(&self.who)?;
        let settled = self
            .connection
            .execute(&format!("UPDATE lifecycle_effects SET {set} WHERE key = ?1 AND stage = 'leased' AND lessee = ?2"), params![key, who, at.unix_millis(), note])
            .map_err(sql)?;
        Ok(settled == 1)
    }

}

/// Requeues each lease of `kind` whose lessee is gone or whose hold ran out, and retires each process-bound effect of
/// `kind` whose committing process is gone.
fn recover(transaction: &Transaction, (who, liveness): (&Owner, &dyn Liveness), kind: &str, at: Timestamp) -> NorthResult<()> {
        let mut statement = transaction.prepare("SELECT effect_id, delivery, owner, stage, lessee, lease_until_ms FROM lifecycle_effects WHERE kind = ?1 AND stage IN ('pending', 'leased')").map_err(sql)?;
        let rows = statement
            .query_map([kind], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<i64>>(5)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let gone = |owner: &Owner| owner != who && liveness.alive(owner) == Some(false);
        for (id, delivery, owner, stage, lessee, lease_until_ms) in rows {
            let owner: Owner = serde_json::from_str(&owner)?;
            if Delivery::parse(&delivery)? == Delivery::ProcessBound && gone(&owner) {
                transaction
                    .execute(
                        "UPDATE lifecycle_effects SET stage = 'retired', settled_ms = ?2, note = ?3, lessee = NULL, lease_until_ms = NULL WHERE effect_id = ?1",
                        params![id, at.unix_millis(), format!("its process, {owner}, is gone")],
                    )
                    .map_err(sql)?;
                continue;
            }
            let lessee: Option<Owner> = lessee.map(|lessee| serde_json::from_str(&lessee)).transpose()?;
            let lapsed = lessee.as_ref().is_some_and(gone) || lease_until_ms.is_some_and(|until| until < at.unix_millis());
            if stage == Stage::Leased.name() && lapsed {
                let note = lessee.map_or_else(|| "its lease ran out".to_owned(), |lessee| format!("its lease by {lessee} lapsed"));
                transaction.execute("UPDATE lifecycle_effects SET stage = 'pending', note = ?2, lessee = NULL, lease_until_ms = NULL WHERE effect_id = ?1", params![id, note]).map_err(sql)?;
            }
        }
        Ok(())
    }

/// Queues `effects` for the move `who` just wrote to `machine`/`instance`, skipping a key already queued.
pub(super) fn queue(transaction: &Transaction, machine: &str, instance: &str, who: &Owner, effects: &[Effect], at: Timestamp) -> NorthResult<()> {
    let owner = serde_json::to_string(who)?;
    for effect in effects {
        transaction
            .execute(
                "INSERT INTO lifecycle_effects(machine, instance, kind, key, payload, delivery, owner, stage, ready_ms, created_ms) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?8) \
                 ON CONFLICT(key) DO NOTHING",
                params![machine, instance, effect.kind, effect.key, serde_json::to_string(&effect.payload)?, effect.delivery.name(), owner, at.unix_millis()],
            )
            .map_err(sql)?;
    }
    Ok(())
}

/// Drops the settled effects of `machine` that settled before `before_ms`.
pub(super) fn prune(transaction: &Transaction, machine: &str, before_ms: i64) -> NorthResult<()> {
    transaction.execute("DELETE FROM lifecycle_effects WHERE machine = ?1 AND stage IN ('delivered', 'retired') AND settled_ms < ?2", params![machine, before_ms]).map_err(sql)?;
    Ok(())
}
