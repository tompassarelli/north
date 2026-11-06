use std::collections::HashMap;
use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension};

use crate::error::{GraphError, Result};
use crate::key::{Key, atom_key, decode_key, triple_key};
use crate::term::Term;

#[derive(Default)]
struct Ephemeral {
    ids: HashMap<Term, i64>,
    terms: Vec<Rc<Term>>,
}

pub struct Terms<'c> {
    pub(super) connection: &'c Connection,
    decoded: HashMap<i64, Rc<Term>>,
    pub(super) found: HashMap<Vec<u8>, i64>,
    ephemeral: Ephemeral,
}

impl<'c> Terms<'c> {
    pub fn new(connection: &'c Connection) -> Self {
        Self { connection, decoded: HashMap::new(), found: HashMap::new(), ephemeral: Ephemeral::default() }
    }

    pub fn stored_id(&mut self, key: &[u8]) -> Result<Option<i64>> {
        if let Some(known) = self.found.get(key) {
            return Ok(Some(*known));
        }
        let id: Option<i64> = self.connection.prepare_cached("SELECT id FROM terms WHERE key = ?1")?.query_row([key], |row| row.get(0)).optional()?;
        if let Some(id) = id {
            self.found.insert(key.to_vec(), id);
        }
        Ok(id)
    }

    pub fn id_of(&mut self, term: &Term) -> Result<i64> {
        match self.stored_id_of(term)? {
            Some(id) => Ok(id),
            None => Ok(self.ephemeral_id(term)),
        }
    }

    pub fn stored_id_of(&mut self, term: &Term) -> Result<Option<i64>> {
        let key = match term {
            Term::Atom(atom) => atom_key(atom),
            Term::Triple(triple) => {
                let mut children = [0_i64; 3];
                for (slot, child) in children.iter_mut().zip([&triple.t1, &triple.t2, &triple.t3]) {
                    match self.stored_id_of(child)? {
                        Some(id) => *slot = id,
                        None => return Ok(None),
                    }
                }
                triple_key(children)
            }
        };
        self.stored_id(&key)
    }

    fn ephemeral_id(&mut self, term: &Term) -> i64 {
        if let Some(id) = self.ephemeral.ids.get(term) {
            return *id;
        }
        self.ephemeral.terms.push(Rc::new(term.clone()));
        let id = -i64::try_from(self.ephemeral.terms.len()).unwrap_or(i64::MAX);
        self.ephemeral.ids.insert(term.clone(), id);
        id
    }

    pub fn term(&mut self, id: i64) -> Result<Rc<Term>> {
        if id < 0 {
            let index = usize::try_from(-(id + 1)).unwrap_or(usize::MAX);
            return self.ephemeral.terms.get(index).cloned().ok_or_else(|| GraphError::Corrupt(format!("unknown computed term {id}")));
        }
        if let Some(known) = self.decoded.get(&id) {
            return Ok(Rc::clone(known));
        }
        let key: Vec<u8> = self
            .connection
            .prepare_cached("SELECT key FROM terms WHERE id = ?1")?
            .query_row([id], |row| row.get(0))
            .optional()?
            .ok_or_else(|| GraphError::Corrupt(format!("term {id} does not exist")))?;
        let term = match decode_key(&key)? {
            Key::Atom(atom) => Term::Atom(atom),
            Key::Triple([t1, t2, t3]) => Term::triple(self.term(t1)?.as_ref().clone(), self.term(t2)?.as_ref().clone(), self.term(t3)?.as_ref().clone()),
        };
        let term = Rc::new(term);
        self.decoded.insert(id, Rc::clone(&term));
        Ok(term)
    }
}
