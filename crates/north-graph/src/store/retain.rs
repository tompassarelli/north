use std::time::Instant;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter};

use super::resolve::Terms;
use super::{Graph, HORIZON};
use crate::error::Result;
use crate::term::Term;

const MARK: &str = "mark/";
const MARK_SPACING_SECONDS: i64 = 3600;
const DAY_SECONDS: i64 = 86_400;
const UNITS_PER_CHUNK: usize = 5_000;
const TERMS_PER_CHUNK: usize = 10_000;
const PAGES_PER_STEP: i64 = 2_048;
const INCREMENTAL: i64 = 2;

#[derive(Clone, Debug)]
pub struct Retention {
    pub retired: Vec<String>,
    pub horizon_days: i64,
    pub now: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Retained {
    pub horizon: Option<i64>,
    pub occurrences: u64,
    pub withdrawals: u64,
    pub terms: u64,
    pub transactions: u64,
    pub longest_ms: u128,
    pub delete_ms: u128,
    pub full_vacuum: bool,
    pub vacuum_ms: u128,
}

impl Retained {
    fn timed<T>(&mut self, write: impl FnOnce() -> Result<T>) -> Result<T> {
        let started = Instant::now();
        let written = write()?;
        self.transactions += 1;
        self.longest_ms = self.longest_ms.max(started.elapsed().as_millis());
        Ok(written)
    }
}

impl Graph {
    pub fn mark(&self, now: i64) -> Result<()> {
        let recent: Option<i64> = self
            .connection
            .query_row(&format!("SELECT 1 FROM meta WHERE key >= '{MARK}' AND key < '{MARK}~' AND CAST(substr(key, {}) AS INTEGER) > ?1", MARK.len() + 1), [now - MARK_SPACING_SECONDS], |row| row.get(0))
            .optional()?;
        if recent.is_none() {
            self.connection.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, (SELECT COALESCE(MAX(seq), 0) FROM occurrences))",
                [format!("{MARK}{now}")],
            )?;
        }
        Ok(())
    }

    pub fn horizon(&self) -> Result<Option<i64>> {
        horizon(&self.connection)
    }

    pub fn retain(&mut self, retention: &Retention) -> Result<Retained> {
        let mut retained = Retained::default();
        let started = Instant::now();
        self.mark(retention.now)?;
        let cut = cut(&self.connection, retention)?;
        if let Some(cut) = cut {
            retained.timed(|| raise_horizon(&self.connection, cut))?;
        }
        retained.horizon = horizon(&self.connection)?;
        let units = doom_occurrences(&self.connection, &retention.retired, cut)?;
        for chunk in units.chunks(UNITS_PER_CHUNK) {
            let (withdrawals, occurrences) = retained.timed(|| delete_units(&self.connection, chunk))?;
            retained.withdrawals += withdrawals;
            retained.occurrences += occurrences;
        }
        self.connection.execute_batch("DROP TABLE temp.doomed")?;
        let mut reach = Reach::snapshot(&self.connection)?;
        let candidates = reach.candidates(&self.connection)?;
        for chunk in candidates.chunks(TERMS_PER_CHUNK) {
            retained.terms += retained.timed(|| reach.delete(&self.connection, chunk))?;
        }
        retained.delete_ms = started.elapsed().as_millis();
        let started = Instant::now();
        retained.full_vacuum = self.reclaim()?;
        retained.vacuum_ms = started.elapsed().as_millis();
        Ok(retained)
    }

    fn reclaim(&self) -> Result<bool> {
        let mode: i64 = self.connection.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
        let full = mode != INCREMENTAL;
        if full {
            self.connection.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
        } else {
            while self.connection.query_row("PRAGMA freelist_count", [], |row| row.get::<_, i64>(0))? > 0 {
                self.connection.execute_batch(&format!("PRAGMA incremental_vacuum({PAGES_PER_STEP})"))?;
            }
        }
        self.connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(full)
    }
}

pub(crate) fn horizon(connection: &Connection) -> Result<Option<i64>> {
    Ok(connection.query_row("SELECT value FROM meta WHERE key = ?1", [HORIZON], |row| row.get(0)).optional()?)
}

fn cut(connection: &Connection, retention: &Retention) -> Result<Option<i64>> {
    let oldest = retention.now - retention.horizon_days * DAY_SECONDS;
    let marked: Option<i64> = connection.query_row(
        &format!("SELECT MAX(value) FROM meta WHERE key >= '{MARK}' AND key < '{MARK}~' AND CAST(substr(key, {}) AS INTEGER) <= ?1", MARK.len() + 1),
        [oldest],
        |row| row.get(0),
    )?;
    Ok(match (marked, horizon(connection)?) {
        (Some(marked), Some(kept)) => Some(marked.max(kept)),
        (marked, kept) => marked.or(kept),
    })
}

fn raise_horizon(connection: &Connection, cut: i64) -> Result<()> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    transaction.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = MAX(value, ?2)", params![HORIZON, cut])?;
    transaction.execute(&format!("DELETE FROM meta WHERE key >= '{MARK}' AND key < '{MARK}~' AND value < ?1"), [cut])?;
    Ok(transaction.commit()?)
}

fn doom_occurrences(connection: &Connection, retired: &[String], cut: Option<i64>) -> Result<Vec<i64>> {
    connection.execute_batch("CREATE TEMP TABLE doomed (seq INTEGER PRIMARY KEY, unit INTEGER NOT NULL); CREATE INDEX temp.doomed_unit ON doomed (unit);")?;
    let mut terms = Terms::new(connection);
    let writers: Vec<i64> = retired.iter().map(|writer| terms.stored_id_of(&Term::string(writer.as_str()))).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect();
    let retract = terms.stored_id_of(&Term::keyword("retract"))?.unwrap_or(0);
    let pairs = "SELECT a.seq AS a, r.seq AS r FROM withdrawals w JOIN occurrences r ON r.coord = w.retraction JOIN occurrences a ON a.coord = w.assertion";
    let failed = format!("SELECT seq FROM occurrences WHERE action = {retract} AND coord NOT IN (SELECT retraction FROM withdrawals)");
    let mut picks = Vec::new();
    if !writers.is_empty() {
        let listed = vec!["?"; writers.len()].join(", ");
        picks.push((
            format!("WITH p AS ({pairs} WHERE r.prop NOT IN (SELECT prop FROM live_props) AND (r.writer IN ({listed}) OR a.writer IN ({listed}))) SELECT a, r FROM p UNION ALL SELECT r, r FROM p"),
            [writers.as_slice(), writers.as_slice()].concat(),
        ));
        picks.push((format!("SELECT seq, seq FROM ({failed} AND writer IN ({listed}) AND prop NOT IN (SELECT prop FROM live_props))"), writers));
    }
    if let Some(cut) = cut {
        picks.push((format!("WITH p AS ({pairs} WHERE r.seq < ?) SELECT a, r FROM p UNION ALL SELECT r, r FROM p"), vec![cut]));
        picks.push((format!("SELECT seq, seq FROM ({failed} AND seq < ?)"), vec![cut]));
    }
    for (select, bound) in picks {
        connection.execute(&format!("INSERT OR IGNORE INTO temp.doomed (seq, unit) {select}"), params_from_iter(bound))?;
    }
    let mut statement = connection.prepare("SELECT DISTINCT unit FROM temp.doomed ORDER BY unit")?;
    let units = statement.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok(units)
}

fn delete_units(connection: &Connection, chunk: &[i64]) -> Result<(u64, u64)> {
    let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else { return Ok((0, 0)) };
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    // SQLite's engine gives a new row max(seq) + 1, so deleting the head would hand a later occurrence a seq a reader already saw.
    transaction.execute("DELETE FROM temp.doomed WHERE unit = (SELECT unit FROM temp.doomed WHERE seq = (SELECT MAX(seq) FROM occurrences))", [])?;
    let withdrawals = transaction.execute(
        "DELETE FROM withdrawals WHERE retraction IN (SELECT o.coord FROM temp.doomed d JOIN occurrences o ON o.seq = d.seq WHERE d.unit BETWEEN ?1 AND ?2)",
        [first, last],
    )?;
    let occurrences = transaction.execute("DELETE FROM occurrences WHERE seq IN (SELECT seq FROM temp.doomed WHERE unit BETWEEN ?1 AND ?2)", [first, last])?;
    transaction.commit()?;
    Ok((u64::try_from(withdrawals).unwrap_or(0), u64::try_from(occurrences).unwrap_or(0)))
}

pub(crate) struct Reach {
    kept: Vec<bool>,
    children: Vec<[i64; 3]>,
    seen_seq: i64,
    seen_term: i64,
}

impl Reach {
    pub(crate) fn snapshot(connection: &Connection) -> Result<Self> {
        let snapshot = connection.unchecked_transaction()?;
        let seen_seq: i64 = snapshot.query_row("SELECT COALESCE(MAX(seq), 0) FROM occurrences", [], |row| row.get(0))?;
        let seen_term: i64 = snapshot.query_row("SELECT COALESCE(MAX(id), 0) FROM terms", [], |row| row.get(0))?;
        let top = usize::try_from(seen_term).unwrap_or(0);
        let mut reach = Self { kept: vec![false; top + 1], children: vec![[0; 3]; top + 1], seen_seq, seen_term };
        {
            let mut statement = snapshot.prepare("SELECT id, t1, t2, t3 FROM quoted")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                if let Some(slot) = reach.slot(row.get(0)?) {
                    reach.children[slot] = [row.get(1)?, row.get(2)?, row.get(3)?];
                }
            }
        }
        for sql in ["SELECT coord, action, prop, writer FROM occurrences", "SELECT prop, t1, t2, t3 FROM live_props"] {
            let mut statement = snapshot.prepare(sql)?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                for column in 0..4 {
                    if let Some(slot) = reach.slot(row.get(column)?) {
                        reach.kept[slot] = true;
                    }
                }
            }
        }
        for slot in (1..=top).rev() {
            if reach.kept[slot] {
                for child in reach.children[slot] {
                    if let Some(child) = reach.slot(child) {
                        reach.kept[child] = true;
                    }
                }
            }
        }
        Ok(reach)
    }

    fn slot(&self, id: i64) -> Option<usize> {
        usize::try_from(id).ok().filter(|slot| *slot > 0 && *slot < self.kept.len())
    }

    pub(crate) fn candidates(&self, connection: &Connection) -> Result<Vec<i64>> {
        let mut statement = connection.prepare("SELECT id FROM terms WHERE id <= ?1 ORDER BY id DESC")?;
        let ids = statement.query_map([self.seen_term], |row| row.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids.into_iter().filter(|id| self.slot(*id).is_some_and(|slot| !self.kept[slot])).collect())
    }

    fn keep(&mut self, connection: &Connection, id: i64) -> Result<()> {
        let children = match self.slot(id) {
            Some(slot) if self.kept[slot] => return Ok(()),
            Some(slot) => {
                self.kept[slot] = true;
                Some(self.children[slot])
            }
            None => connection.prepare_cached("SELECT t1, t2, t3 FROM quoted WHERE id = ?1")?.query_row([id], |row| Ok([row.get(0)?, row.get(1)?, row.get(2)?])).optional()?,
        };
        for child in children.into_iter().flatten().filter(|child| *child > 0) {
            self.keep(connection, child)?;
        }
        Ok(())
    }

    fn rescue(&mut self, connection: &Connection) -> Result<()> {
        let mut referenced = Vec::new();
        {
            let mut statement = connection.prepare_cached("SELECT seq, coord, action, prop, writer FROM occurrences WHERE seq > ?1")?;
            let mut rows = statement.query([self.seen_seq])?;
            while let Some(row) = rows.next()? {
                self.seen_seq = self.seen_seq.max(row.get(0)?);
                referenced.extend([row.get::<_, i64>(1)?, row.get(2)?, row.get(3)?, row.get(4)?]);
            }
            let mut statement = connection.prepare_cached("SELECT id FROM quoted WHERE id > ?1")?;
            let mut rows = statement.query([self.seen_term])?;
            while let Some(row) = rows.next()? {
                referenced.push(row.get(0)?);
            }
        }
        for id in referenced {
            self.keep(connection, id)?;
        }
        Ok(())
    }

    pub(crate) fn delete(&mut self, connection: &Connection, chunk: &[i64]) -> Result<u64> {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        self.rescue(&transaction)?;
        transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS doomed_terms (id INTEGER PRIMARY KEY); DELETE FROM temp.doomed_terms;")?;
        {
            let mut insert = transaction.prepare_cached("INSERT INTO temp.doomed_terms (id) VALUES (?1)")?;
            for id in chunk.iter().filter(|id| self.slot(**id).is_some_and(|slot| !self.kept[slot])) {
                insert.execute([id])?;
            }
        }
        transaction.execute("DELETE FROM quoted WHERE id IN (SELECT id FROM temp.doomed_terms)", [])?;
        let deleted = transaction.execute("DELETE FROM terms WHERE id IN (SELECT id FROM temp.doomed_terms)", [])?;
        transaction.commit()?;
        Ok(u64::try_from(deleted).unwrap_or(0))
    }
}
