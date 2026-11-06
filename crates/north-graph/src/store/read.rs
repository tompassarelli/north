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
