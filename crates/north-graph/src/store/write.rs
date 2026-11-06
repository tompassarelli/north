use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::resolve::Terms;
use super::Graph;
use crate::error::{GraphError, Result};
use crate::key::{atom_key, triple_key};
use crate::term::{MAX_TERM_DEPTH, Term, Triple};

const SPACE: &str = "graph";
const TX_SEQUENCE: &str = "kernel/tx-sequence";
const OP_ORDINAL: &str = "kernel/op-ordinal";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionKind {
    Assert,
    Retract,
}

impl ActionKind {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Assert => "assert",
            Self::Retract => "retract",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Action {
    pub kind: ActionKind,
    pub triple: Triple,
}

impl Action {
    pub fn assert(triple: Triple) -> Self {
        Self { kind: ActionKind::Assert, triple }
    }

    pub fn retract(triple: Triple) -> Self {
        Self { kind: ActionKind::Retract, triple }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub kind: ActionKind,
    pub coordinate: Term,
    pub state_changed: bool,
    pub withdrew: Option<Term>,
}

impl Terms<'_> {
    pub(super) fn intern(&mut self, term: &Term) -> Result<i64> {
        match term {
            Term::Atom(atom) => self.intern_key(&atom_key(atom)),
            Term::Triple(triple) => {
                let children = [self.intern(&triple.t1)?, self.intern(&triple.t2)?, self.intern(&triple.t3)?];
                self.intern_key(&triple_key(children))
            }
        }
    }

    fn intern_key(&mut self, key: &[u8]) -> Result<i64> {
        match self.stored_id(key)? {
            Some(id) => Ok(id),
            None => self.insert_key(key),
        }
    }

    fn insert_key(&mut self, key: &[u8]) -> Result<i64> {
        self.connection.prepare_cached("INSERT INTO terms (key) VALUES (?1)")?.execute([key])?;
        let id = self.connection.last_insert_rowid();
        self.found.insert(key.to_vec(), id);
        Ok(id)
    }
}

impl Graph {
    pub fn tell(&mut self, writer: &str, triple: Triple) -> Result<Receipt> {
        Ok(self.apply(writer, &[Action::assert(triple)])?.remove(0))
    }

    pub fn retract(&mut self, writer: &str, triple: Triple) -> Result<Receipt> {
        Ok(self.apply(writer, &[Action::retract(triple)])?.remove(0))
    }

    pub fn apply(&mut self, writer: &str, actions: &[Action]) -> Result<Vec<Receipt>> {
        if let Some(depth) = actions.iter().map(|action| action.triple.depth()).find(|depth| *depth > MAX_TERM_DEPTH) {
            return Err(GraphError::Invalid(format!("a term nests {depth} deep; the limit is {MAX_TERM_DEPTH}")));
        }
        let transaction = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let receipts = Writer::new(&transaction, writer)?.run(actions)?;
        transaction.commit()?;
        Ok(receipts)
    }
}

struct Writer<'t> {
    connection: &'t Transaction<'t>,
    terms: Terms<'t>,
    writer: &'t str,
    assert_id: i64,
    retract_id: i64,
    op_ordinal_id: i64,
    transaction_coordinate: (Term, i64),
}

impl<'t> Writer<'t> {
    fn new(connection: &'t Transaction<'t>, writer: &'t str) -> Result<Self> {
        let mut terms = Terms::new(connection);
        let sequence: i64 = connection.query_row("SELECT value FROM meta WHERE key = 'tx'", [], |row| row.get(0)).optional()?.unwrap_or(0) + 1;
        connection.execute("INSERT INTO meta (key, value) VALUES ('tx', ?1) ON CONFLICT (key) DO UPDATE SET value = ?1", [sequence])?;
        let coordinate = Term::triple(SPACE, Term::keyword(TX_SEQUENCE), sequence);
        let coordinate_id = terms.intern(&coordinate)?;
        Ok(Self {
            connection,
            writer,
            assert_id: terms.intern(&Term::keyword(ActionKind::Assert.keyword()))?,
            retract_id: terms.intern(&Term::keyword(ActionKind::Retract.keyword()))?,
            op_ordinal_id: terms.intern(&Term::keyword(OP_ORDINAL))?,
            transaction_coordinate: (coordinate, coordinate_id),
            terms,
        })
    }

    fn run(&mut self, actions: &[Action]) -> Result<Vec<Receipt>> {
        let mut receipts = Vec::with_capacity(actions.len());
        for (ordinal, action) in actions.iter().enumerate() {
            let ordinal = i64::try_from(ordinal).unwrap_or(i64::MAX);
            let ordinal_id = self.terms.intern(&Term::int(ordinal))?;
            let coordinate_id = self.terms.intern_key(&triple_key([self.transaction_coordinate.1, self.op_ordinal_id, ordinal_id]))?;
            let coordinate = Term::triple(self.transaction_coordinate.0.clone(), Term::keyword(OP_ORDINAL), ordinal);
            let proposition = self.terms.intern(&Term::from(action.triple.clone()))?;
            let (state_changed, withdrew) = match action.kind {
                ActionKind::Assert => (self.assert(coordinate_id, proposition, &action.triple)?, None),
                ActionKind::Retract => self.retract(coordinate_id, proposition)?,
            };
            receipts.push(Receipt { kind: action.kind, coordinate, state_changed, withdrew });
        }
        Ok(receipts)
    }

    fn record(&self, coordinate: i64, action: i64, proposition: i64, live: bool) -> Result<i64> {
        self.connection
            .prepare_cached("INSERT INTO occurrences (coord, action, prop, writer, live) VALUES (?1, ?2, ?3, ?4, ?5)")?
            .execute(params![coordinate, action, proposition, self.writer, live])?;
        Ok(self.connection.last_insert_rowid())
    }

    fn assert(&mut self, coordinate: i64, proposition: i64, triple: &Triple) -> Result<bool> {
        self.record(coordinate, self.assert_id, proposition, true)?;
        let [t1, t2, t3] = [&triple.t1, &triple.t2, &triple.t3].map(|term| self.terms.intern(term));
        self.connection
            .prepare_cached("INSERT INTO live_props (prop, t1, t2, t3, n) VALUES (?1, ?2, ?3, ?4, 1) ON CONFLICT (prop) DO UPDATE SET n = n + 1")?
            .execute(params![proposition, t1?, t2?, t3?])?;
        Ok(true)
    }

    fn retract(&mut self, coordinate: i64, proposition: i64) -> Result<(bool, Option<Term>)> {
        self.record(coordinate, self.retract_id, proposition, false)?;
        let target: Option<(i64, i64)> = self
            .connection
            .prepare_cached("SELECT seq, coord FROM occurrences WHERE prop = ?1 AND live = 1 ORDER BY seq DESC LIMIT 1")?
            .query_row([proposition], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        let Some((sequence, assertion)) = target else {
            return Ok((false, None));
        };
        self.connection.prepare_cached("UPDATE occurrences SET live = 0 WHERE seq = ?1")?.execute([sequence])?;
        self.connection.prepare_cached("INSERT INTO withdrawals (retraction, assertion) VALUES (?1, ?2)")?.execute([coordinate, assertion])?;
        self.connection.prepare_cached("UPDATE live_props SET n = n - 1 WHERE prop = ?1")?.execute([proposition])?;
        self.connection.prepare_cached("DELETE FROM live_props WHERE prop = ?1 AND n = 0")?.execute([proposition])?;
        Ok((true, Some(self.terms.term(assertion)?.as_ref().clone())))
    }
}
