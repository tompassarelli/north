use std::collections::BTreeSet;

use rusqlite::params;

use super::{Field, field_ids};
use crate::error::Result;
use crate::key::{Key, decode_key};
use crate::store::resolve::Terms;
use crate::term::Atom;

#[derive(Default)]
pub struct Reindex {
    subjects: BTreeSet<i64>,
}

impl Reindex {
    pub fn mark(&mut self, subject: i64) {
        self.subjects.insert(subject);
    }

    pub fn run(&mut self, terms: &mut Terms) -> Result<()> {
        if self.subjects.is_empty() {
            return Ok(());
        }
        let fields = field_ids(terms)?;
        let connection = terms.connection;
        for subject in std::mem::take(&mut self.subjects) {
            connection.prepare_cached("DELETE FROM text_stems WHERE rowid = ?1")?.execute([subject])?;
            let mut name = Vec::new();
            let mut body = Vec::new();
            let mut statement = connection.prepare_cached("SELECT p.t2, t.key FROM live_props p JOIN terms t ON t.id = p.t3 WHERE p.t1 = ?1 ORDER BY p.prop")?;
            for row in statement.query_map([subject], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)))? {
                let (predicate, key) = row?;
                let Some((_, field)) = fields.iter().find(|(id, _)| *id == predicate) else { continue };
                let Key::Atom(Atom::String(text)) = decode_key(&key)? else { continue };
                match field {
                    Field::Name => name.push(text),
                    Field::Body => body.push(text),
                }
            }
            if !name.is_empty() || !body.is_empty() {
                connection.prepare_cached("INSERT INTO text_stems (rowid, name, body) VALUES (?1, ?2, ?3)")?.execute(params![subject, name.join("\n"), body.join("\n")])?;
            }
        }
        Ok(())
    }
}
