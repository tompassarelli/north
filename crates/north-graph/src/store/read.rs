use super::resolve::Terms;
use super::Graph;
use crate::error::Result;
use crate::term::{Term, Triple};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Live {
    pub triple: Triple,
    pub occurrences: u64,
}

impl Graph {
    pub fn show(&self, t1: &Term) -> Result<Vec<Live>> {
        let snapshot = self.connection().unchecked_transaction()?;
        let mut terms = Terms::new(&snapshot);
        let Some(first) = terms.stored_id_of(t1)? else {
            return Ok(Vec::new());
        };
        let mut statement = snapshot.prepare_cached("SELECT t2, t3, n FROM live_props WHERE t1 = ?1 ORDER BY prop")?;
        let rows = statement.query_map([first], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))?;
        let mut live = Vec::new();
        for row in rows {
            let (t2, t3, occurrences) = row?;
            let triple = Triple::new(t1.clone(), terms.term(t2)?.as_ref().clone(), terms.term(t3)?.as_ref().clone());
            live.push(Live { triple, occurrences: occurrences.unsigned_abs() });
        }
        Ok(live)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Told {
    pub writer: String,
    pub triple: Triple,
}

impl Told {
    pub fn new(writer: impl Into<String>, t1: impl Into<Term>, t2: impl Into<Term>, t3: impl Into<Term>) -> Self {
        Self { writer: writer.into(), triple: Triple::new(t1, t2, t3) }
    }
}

const MENTIONS: &str = "SELECT DISTINCT writer, t1, t2, t3 FROM assertions WHERE t1 = ?1 OR t3 = ?1
    UNION
    SELECT DISTINCT a.writer, a.t1, a.t2, a.t3 FROM quoted q JOIN assertions a ON a.t3 = q.id WHERE q.t1 = ?1 OR q.t3 = ?1
    LIMIT ?2";

fn told_from(terms: &mut Terms, [writer, t1, t2, t3]: [i64; 4]) -> Result<Told> {
    let writer = match terms.term(writer)?.as_ref() {
        Term::Atom(crate::term::Atom::String(name)) => name.clone(),
        other => other.to_string(),
    };
    let triple = Triple::new(terms.term(t1)?.as_ref().clone(), terms.term(t2)?.as_ref().clone(), terms.term(t3)?.as_ref().clone());
    Ok(Told { writer, triple })
}

impl Graph {
    pub fn told(&self) -> Result<Vec<Told>> {
        let snapshot = self.connection().unchecked_transaction()?;
        let mut terms = Terms::new(&snapshot);
        let mut statement = snapshot.prepare("SELECT DISTINCT writer, t1, t2, t3 FROM assertions")?;
        let rows = statement.query_map([], |row| Ok([row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?]))?;
        rows.map(|row| told_from(&mut terms, row?)).collect()
    }

    pub fn mentions(&self, term: &Term, limit: usize) -> Result<Vec<Told>> {
        let snapshot = self.connection().unchecked_transaction()?;
        let mut terms = Terms::new(&snapshot);
        let Some(id) = terms.stored_id_of(term)? else {
            return Ok(Vec::new());
        };
        let mut statement = snapshot.prepare_cached(MENTIONS)?;
        let rows = statement.query_map([id, i64::try_from(limit).unwrap_or(i64::MAX)], |row| Ok([row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?]))?;
        rows.map(|row| told_from(&mut terms, row?)).collect()
    }
}
