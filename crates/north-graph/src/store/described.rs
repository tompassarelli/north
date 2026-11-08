use super::Graph;
use super::read::Told;
use super::resolve::Terms;
use crate::error::Result;
use crate::term::{Atom, Term, Triple};

const DESCRIBED: &str = "SELECT DISTINCT o.writer, p.t1, p.t2, p.t3 FROM live_props k
    JOIN live_props p ON p.t1 = k.t1
    JOIN occurrences o ON o.prop = p.prop AND o.live = 1
    WHERE k.t2 = ?1 AND k.t3 = ?2";

impl Graph {
    /// Every live told fact about each subject that holds `(predicate object)`.
    pub fn described(&self, predicate: &Term, object: &Term) -> Result<Vec<Told>> {
        let snapshot = self.connection().unchecked_transaction()?;
        let mut terms = Terms::new(&snapshot);
        let (Some(predicate), Some(object)) = (terms.stored_id_of(predicate)?, terms.stored_id_of(object)?) else {
            return Ok(Vec::new());
        };
        let mut statement = snapshot.prepare(DESCRIBED)?;
        let rows = statement.query_map([predicate, object], |row| Ok([row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?]))?;
        let mut told = Vec::new();
        for row in rows {
            let [writer, t1, t2, t3] = row?;
            let writer = match terms.term(writer)?.as_ref() {
                Term::Atom(Atom::String(name)) => name.clone(),
                other => other.to_string(),
            };
            let triple = Triple::new(terms.term(t1)?.as_ref().clone(), terms.term(t2)?.as_ref().clone(), terms.term(t3)?.as_ref().clone());
            told.push(Told { writer, triple });
        }
        Ok(told)
    }
}
