use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::SystemTime;

use rustix::fs::{FlockOperation, flock};

use crate::error::{NorthError, NorthResult};
use crate::fleet::store::FleetRow;
use crate::fleet::transcript::stream_path;
use crate::fleet::unaccepted::Unaccepted;

use super::Fleet;
use super::senders::signed;

const DELIVERED: &str = "delivered";
const LOCK: &str = ".lock";
const HANDING: &str = "handing";

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
        let handing = journal.join(format!("{}.{HANDING}", row.id));
        if handing.exists() {
            if self.took(row)? {
                fs::remove_file(&handing)?;
                return Ok(HandedIn::Already);
            }
        } else if entry.exists() {
            return Ok(HandedIn::Already);
        }
        File::create(&handing)?;
        File::create(&entry)?;
        let handed = self.deliver(row);
        if handed.is_err() && let Err(error) = fs::remove_file(&entry) {
            eprintln!("north fleet: row {} was not delivered and its journal entry was not cleared: {error}", row.id);
        }
        if let Err(error) = fs::remove_file(&handing) {
            eprintln!("north fleet: row {} hand-in marker was not cleared: {error}", row.id);
        }
        handed.map(|()| HandedIn::Delivered)
    }

    pub fn delivered(&self, name: &str) -> NorthResult<Vec<String>> {
        let journal = self.session_dir(name)?.join(DELIVERED);
        let Ok(entries) = fs::read_dir(&journal) else { return Ok(Vec::new()) };
        let names: Vec<String> = entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        let marker = format!(".{HANDING}");
        let handing: Vec<&str> = names.iter().filter_map(|name| name.strip_suffix(&marker)).collect();
        Ok(names.iter().filter(|name| !name.contains('.') && !handing.contains(&name.as_str())).cloned().collect())
    }

    pub fn prune_delivered(&self, before: SystemTime) -> NorthResult<usize> {
        let mut pruned = 0;
        for record in self.sessions()? {
            let journal = self.session_dir(&record.name)?.join(DELIVERED);
            let Ok(entries) = fs::read_dir(&journal) else { continue };
            for entry in entries.flatten() {
                let stale = entry.metadata().and_then(|metadata| metadata.modified()).is_ok_and(|modified| modified < before);
                if stale && !entry.file_name().to_string_lossy().contains('.') {
                    fs::remove_file(entry.path())?;
                    pruned += 1;
                }
            }
        }
        Ok(pruned)
    }

    fn took(&self, row: &FleetRow) -> NorthResult<bool> {
        let record = self.record(&row.to)?;
        let id = row.prompt_id();
        Ok(Unaccepted::holds(&record.host_dir, &id) || streamed(&stream_path(&record.host_dir), &id)?)
    }

    fn deliver(&self, row: &FleetRow) -> NorthResult<()> {
        let signer = if row.from == self.host { row.sender.clone() } else { Some(row.signer()) };
        self.prompt(&row.to, &row.prompt_id(), &signed(signer.as_deref(), &row.text), row.delivery())?;
        if let Some(signer) = signer
            && let Err(error) = self.note_sender(&row.to, &signer)
        {
            eprintln!("north fleet: delivered to {}, but its sender was not recorded: {}", row.to, error.user_message());
        }
        Ok(())
    }
}

fn streamed(stream: &Path, id: &str) -> NorthResult<bool> {
    match fs::read(stream) {
        Ok(bytes) => Ok(bytes.windows(id.len()).any(|window| window == id.as_bytes())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
