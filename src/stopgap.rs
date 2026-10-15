use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::local_store::LocalStore;
use crate::foundation::clock::Timestamp;
use crate::watch;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub what: String,
    pub owner: String,
    pub deadline: Timestamp,
    pub match_tag: String,
    pub replaces: Option<String>,
    pub added_at: Timestamp,
    pub closed_at: Option<Timestamp>,
}

impl Entry {
    pub fn is_open(&self) -> bool {
        self.closed_at.is_none()
    }

    pub fn is_overdue(&self, now: Timestamp) -> bool {
        self.is_open() && now >= self.deadline
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Record {
    Add {
        id: String,
        what: String,
        owner: String,
        deadline: Timestamp,
        match_tag: String,
        replaces: Option<String>,
        at: Timestamp,
    },
    Close {
        id: String,
        at: Timestamp,
    },
}

pub struct Registry {
    path: PathBuf,
}

impl Registry {
    pub fn default_path() -> NorthResult<PathBuf> {
        Ok(LocalStore::default_root()?.join("stopgaps.jsonl"))
    }

    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn add(&self, what: &str, owner: &str, deadline: Timestamp, match_tag: &str, replaces: Option<&str>) -> NorthResult<Entry> {
        for (name, value) in [("--what", what), ("--owner", owner), ("--match", match_tag)] {
            if value.trim().is_empty() { return Err(NorthError::Usage(format!("north stopgap add needs a non-empty {name}"))); }
        }
        let entries = self.list()?;
        let id = format!("sg-{}", entries.len() + 1);
        let at = Timestamp::now().whole_seconds();
        self.append(&Record::Add {
            id: id.clone(), what: what.into(), owner: owner.into(), deadline, match_tag: match_tag.into(),
            replaces: replaces.map(Into::into), at,
        })?;
        Ok(Entry {
            id, what: what.into(), owner: owner.into(), deadline, match_tag: match_tag.into(),
            replaces: replaces.map(Into::into), added_at: at, closed_at: None,
        })
    }

    pub fn close(&self, id: &str) -> NorthResult<Entry> {
        let mut entry = self.list()?.into_iter().find(|entry| entry.id == id)
            .ok_or_else(|| NorthError::Usage(format!("No stopgap {id}")))?;
        if !entry.is_open() { return Err(NorthError::Usage(format!("Stopgap {id} is already closed"))); }
        let at = Timestamp::now().whole_seconds();
        self.append(&Record::Close { id: id.into(), at })?;
        entry.closed_at = Some(at);
        Ok(entry)
    }

    pub fn list(&self) -> NorthResult<Vec<Entry>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let complete = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
        let mut entries = Vec::<Entry>::new();
        for line in bytes[..complete].split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
            match serde_json::from_slice(line)? {
                Record::Add { id, what, owner, deadline, match_tag, replaces, at } => {
                    entries.push(Entry { id, what, owner, deadline, match_tag, replaces, added_at: at, closed_at: None });
                }
                Record::Close { id, at } => {
                    let entry = entries.iter_mut().find(|entry| entry.id == id).ok_or_else(|| NorthError::State(
                        format!("stopgap registry closes unknown {id}")
                    ))?;
                    entry.closed_at.get_or_insert(at);
                }
            }
        }
        Ok(entries)
    }

    pub fn export(&self) -> NorthResult<Vec<watch::Stopgap>> {
        Ok(self.list()?.into_iter().filter(Entry::is_open).map(|entry| watch::Stopgap {
            id: entry.id, what: entry.what, owner: entry.owner, deadline: entry.deadline, match_tag: entry.match_tag,
        }).collect())
    }

    fn append(&self, record: &Record) -> NorthResult<()> {
        if let Some(parent) = self.path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
        }
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).open(&self.path)?;
        file.lock()?;
        repair(&mut file)?;
        let mut line = serde_json::to_vec(record)?;
        line.push(b'\n');
        file.seek(SeekFrom::End(0))?;
        file.write_all(&line)?;
        file.sync_all()?;
        Ok(())
    }
}

fn repair(file: &mut File) -> NorthResult<()> {
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_end(&mut bytes)?;
    if bytes.last().is_none_or(|byte| *byte == b'\n') { return Ok(()); }
    let complete = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
    file.set_len(complete as u64)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::{Invariant, evaluate};

    fn deadline(text: &str) -> Timestamp {
        Timestamp::parse(text).unwrap()
    }

    #[test]
    fn add_list_and_close_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::open(directory.path().join("state/stopgaps.jsonl"));
        assert!(registry.list().unwrap().is_empty());
        assert!(Registry::default_path().unwrap().ends_with("north/stopgaps.jsonl"));
        let first = registry.add("watch lead-a inbox", "proxy", deadline("2026-10-16T12:00:00Z"), "sg-lead-a", Some("north#63")).unwrap();
        let second = registry.add("tail proxy inbox", "proxy", deadline("2026-10-17T12:00:00Z"), "sg-proxy", None).unwrap();
        assert_eq!((first.id.as_str(), second.id.as_str()), ("sg-1", "sg-2"));
        assert_eq!(registry.list().unwrap(), vec![first.clone(), second.clone()]);
        let closed = registry.close("sg-1").unwrap();
        assert!(closed.closed_at.is_some());
        assert!(registry.close("sg-1").is_err());
        assert!(registry.close("sg-9").is_err());
        assert!(registry.add(" ", "proxy", deadline("2026-10-17T12:00:00Z"), "tag", None).is_err());
        let reopened = Registry::open(registry.path());
        assert_eq!(reopened.list().unwrap(), vec![closed, second.clone()]);
        assert_eq!(reopened.export().unwrap().iter().map(|stopgap| stopgap.id.as_str()).collect::<Vec<_>>(), ["sg-2"]);
        assert!(!second.is_overdue(deadline("2026-10-17T11:59:00Z")));
        assert!(second.is_overdue(deadline("2026-10-17T12:00:00Z")));
        let text = fs::read_to_string(registry.path()).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn torn_suffix_is_repaired_before_append() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stopgaps.jsonl");
        let registry = Registry::open(&path);
        let first = registry.add("watch lead-a", "proxy", deadline("2026-10-16T12:00:00Z"), "sg-a", None).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(br#"{"op":"add","id":"sg-2","what":"tor"#).unwrap();
        drop(file);
        assert_eq!(registry.list().unwrap(), vec![first.clone()]);
        let second = registry.add("watch lead-b", "proxy", deadline("2026-10-16T13:00:00Z"), "sg-b", None).unwrap();
        assert_eq!(second.id, "sg-2");
        let reopened = Registry::open(&path);
        assert_eq!(reopened.list().unwrap(), vec![first, second]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("tor\""));
        assert!(text.lines().all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok()));

        let torn = directory.path().join("torn-only.jsonl");
        fs::write(&torn, b"{\"op\":\"add\"").unwrap();
        let registry = Registry::open(&torn);
        assert!(registry.list().unwrap().is_empty());
        let only = registry.add("watch lead-c", "proxy", deadline("2026-10-16T14:00:00Z"), "sg-c", None).unwrap();
        assert_eq!(Registry::open(&torn).list().unwrap(), vec![only]);
    }

    #[test]
    fn overdue_export_fires_i5_in_watch_fixture() {
        let directory = tempfile::tempdir().unwrap();
        let registry = Registry::open(directory.path().join("stopgaps.jsonl"));
        let mut fixture = crate::watch::invariants::fixture();
        let start = fixture.start;
        let overdue = registry.add("tail lead-b status", "proxy", start.plus_seconds(18 * 60), "sg-lead-b", None).unwrap();
        let closed = registry.add("watch org.json", "proxy", start.plus_seconds(5 * 60), "sg-org", None).unwrap();
        registry.close(&closed.id).unwrap();
        fixture.snapshot.stopgaps = registry.export().unwrap();
        let first = crate::watch::invariants::first_fire(&fixture);
        let fired = first.iter().filter(|((invariant, _), _)| *invariant == Invariant::I5).collect::<Vec<_>>();
        assert_eq!(fired.len(), 1);
        let ((_, subject), (minute, alarm)) = fired[0];
        assert_eq!((subject.as_str(), *minute, alarm.tom_level), (overdue.id.as_str(), 18, true));
        assert!(evaluate(&fixture.snapshot, start.plus_seconds(17 * 60)).iter().all(|alarm| alarm.invariant != Invariant::I5));
    }
}
