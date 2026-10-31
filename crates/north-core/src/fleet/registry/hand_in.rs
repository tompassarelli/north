use std::fs::{self, OpenOptions};
use std::io;

use rustix::fs::{FlockOperation, flock};

use crate::error::{NorthError, NorthResult};
use crate::fleet::store::{FleetRow, Spool};

use super::Fleet;
use super::senders::signed;

const DELIVERED: &str = "delivered";
const LOCK: &str = ".lock";

#[derive(Debug, Eq, PartialEq)]
pub enum HandedIn {
    Delivered,
    Already,
}

impl Fleet {
    pub fn hand_in(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        row.validate()?;
        self.record(&row.to)?;
        let journal = self.session_dir(&row.to)?.join(DELIVERED);
        fs::create_dir_all(&journal)?;
        let lock = OpenOptions::new().create(true).append(true).open(journal.join(LOCK))?;
        flock(&lock, FlockOperation::LockExclusive).map_err(|error| NorthError::State(format!("{} delivery lock: {error}", row.to)))?;
        let entry = journal.join(&row.id);
        match OpenOptions::new().write(true).create_new(true).open(&entry) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(HandedIn::Already),
            Err(error) => return Err(error.into()),
        }
        let handed = self.deliver(row);
        if handed.is_err()
            && let Err(error) = fs::remove_file(&entry)
        {
            eprintln!("north fleet: row {} was not delivered and its journal entry was not cleared: {error}", row.id);
        }
        handed.map(|()| HandedIn::Delivered)
    }

    fn deliver(&self, row: &FleetRow) -> NorthResult<()> {
        let signer = if Spool::at(&self.root).holds(&row.id) { row.sender.clone() } else { Some(row.signer()) };
        self.send(&row.to, &signed(signer.as_deref(), &row.text), row.delivery())?;
        if let Some(signer) = signer
            && let Err(error) = self.note_sender(&row.to, &signer)
        {
            eprintln!("north fleet: delivered to {}, but its sender was not recorded: {}", row.to, error.user_message());
        }
        Ok(())
    }
}
