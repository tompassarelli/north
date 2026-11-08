mod index;
mod search;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use super::resolve::Terms;
use crate::error::Result;
use crate::key::atom_key;
use crate::term::{Atom, Term};

pub use search::{TextHit, TextSearch};

pub(super) use index::Reindex;

const VERSION_KEY: &str = "text-index";
const VERSION: i64 = 1;
const TABLES: &str = "
DROP TABLE IF EXISTS text_stems;
CREATE VIRTUAL TABLE text_stems USING fts5(name, body, content='', contentless_delete=1, tokenize='porter unicode61 remove_diacritics 2');
";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    Name,
    Body,
}

const FIELDS: [(&str, Field); 4] = [("title", Field::Name), ("doc-title", Field::Name), ("text", Field::Body), ("doc-text", Field::Body)];

pub(super) fn indexed(predicate: &Term, object: &Term) -> bool {
    matches!(object, Term::Atom(Atom::String(_))) && matches!(predicate, Term::Atom(Atom::Keyword(name)) if FIELDS.iter().any(|(field, _)| field == name))
}

fn field_ids(terms: &mut Terms) -> Result<Vec<(i64, Field)>> {
    let mut ids = Vec::with_capacity(FIELDS.len());
    for (name, field) in FIELDS {
        if let Some(id) = terms.stored_id(&atom_key(&Atom::Keyword(name.to_string())))? {
            ids.push((id, field));
        }
    }
    Ok(ids)
}

pub(super) fn prepare(connection: &mut Connection) -> Result<()> {
    let built = |connection: &Connection| -> Result<Option<i64>> {
        Ok(connection.query_row("SELECT value FROM meta WHERE key = ?1", [VERSION_KEY], |row| row.get(0)).optional()?)
    };
    if built(connection)? == Some(VERSION) {
        return Ok(());
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if built(&transaction)? != Some(VERSION) {
        transaction.execute_batch(TABLES)?;
        let mut reindex = Reindex::default();
        let mut terms = Terms::new(&transaction);
        let ids = field_ids(&mut terms)?;
        for (id, _) in &ids {
            let mut subjects = transaction.prepare_cached("SELECT DISTINCT t1 FROM live_props WHERE t2 = ?1")?;
            for subject in subjects.query_map([id], |row| row.get::<_, i64>(0))? {
                reindex.mark(subject?);
            }
        }
        reindex.run(&mut terms)?;
        transaction.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2", rusqlite::params![VERSION_KEY, VERSION])?;
    }
    transaction.commit()?;
    Ok(())
}
