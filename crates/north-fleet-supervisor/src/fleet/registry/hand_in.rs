use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

use north_session_host::row_delivery::{Phase, RowDelivery, instance};
use rustix::fs::{FlockOperation, flock};

use crate::error::{NorthError, NorthResult};
use crate::fleet::store::FleetRow;
use crate::fleet::transcript::stream_path;
use crate::fleet::unaccepted::Unaccepted;

use super::Fleet;
use super::senders::signed;

const LOCK: &str = ".delivery.lock";

#[derive(Debug, Eq, PartialEq)]
pub enum HandedIn {
    Delivered,
    Already,
}

impl Fleet {
    pub fn hand_in(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        row.validate()?;
        if let Some(heir) = self.heir(&row.to)? {
            return self.hand_in_to_heir(row, &heir);
        }
        self.record(&row.to)?;
        self.rebind_dead_proxy(&row.to)?;
        let _lock = self.delivery_lock(&row.to)?;
        let mut store = self.deliveries()?;
        let handed_in = match store.current::<RowDelivery>(&instance(&row.to, &row.id))?.map(|record| record.state) {
            Some(Phase::Delivered) => return Ok(HandedIn::Already),
            Some(Phase::HandingIn | Phase::Interrupted) if self.took(&row.to, &row.id)? => {
                self.advance(&mut store, &row.to, &row.id, Phase::Delivered, "the session's stream already holds the row")?;
                return Ok(HandedIn::Already);
            }
            _ => self.advance(&mut store, &row.to, &row.id, Phase::HandingIn, "handing the row to the session")?,
        };
        if !handed_in {
            return Ok(HandedIn::Already);
        }
        if let Err(error) = self.deliver(row) {
            self.advance(&mut store, &row.to, &row.id, Phase::Failed, &error.user_message())?;
            return Err(error);
        }
        self.advance(&mut store, &row.to, &row.id, Phase::Delivered, "the session took the row")?;
        Ok(HandedIn::Delivered)
    }

    pub(super) fn took(&self, session: &str, id: &str) -> NorthResult<bool> {
        let record = self.record(session)?;
        let prompt = FleetRow::prompt_id_for(id);
        Ok(Unaccepted::holds(&record.host_dir, &prompt) || streamed(&stream_path(&record.host_dir), &prompt)?)
    }

    pub(super) fn journaled(&self, row: &FleetRow, apply: impl FnOnce() -> NorthResult<()>) -> NorthResult<HandedIn> {
        row.validate()?;
        self.record(&row.to)?;
        let _lock = self.delivery_lock(&row.to)?;
        let mut store = self.deliveries()?;
        if store.current::<RowDelivery>(&instance(&row.to, &row.id))?.is_some() {
            return Ok(HandedIn::Already);
        }
        apply()?;
        self.advance(&mut store, &row.to, &row.id, Phase::Delivered, "the row was applied")?;
        Ok(HandedIn::Delivered)
    }

    fn delivery_lock(&self, name: &str) -> NorthResult<File> {
        let lock = OpenOptions::new().create(true).append(true).open(self.session_dir(name)?.join(LOCK))?;
        flock(&lock, FlockOperation::LockExclusive).map_err(|error| NorthError::State(format!("{name} delivery lock: {error}")))?;
        Ok(lock)
    }

    fn deliver(&self, row: &FleetRow) -> NorthResult<()> {
        let signer = if row.from == self.host { row.sender.clone() } else { row.signer() };
        if let Err(error) = self.note_sender(&row.to, &signer) {
            eprintln!("north fleet: delivering to {}, but its sender was not recorded: {}", row.to, error.user_message());
        }
        self.prompt(&row.to, &row.prompt_id(), &signed(&signer, &row.text), row.delivery())
    }
}

fn streamed(stream: &Path, id: &str) -> NorthResult<bool> {
    match fs::read(stream) {
        Ok(bytes) => Ok(bytes.windows(id.len()).any(|window| window == id.as_bytes())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
