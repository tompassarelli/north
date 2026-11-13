//! The finish record (north#1233): the verdict a session's last finish call got for its acceptance command. The
//! finish call writes it, and the session watcher reads a failed acceptance check from it, never from the lane or from
//! the text of a tool result or a report.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::platform::harness::{DEFAULTS, document};

const STORE_ENV: &str = "NORTH_VERDICT_CACHE_DIR";
const DIR: &str = "finished";
const SWITCH: &str = "finish_acceptance_in_prequeue";
const REMOTE_DIR: &str = "\"${XDG_STATE_HOME:-$HOME/.local/state}/north/verdict-cache\"";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub session: String,
    pub command: String,
    pub head: String,
    pub passed: bool,
    pub at: u64,
}

pub struct FinishRecords(pub PathBuf);

fn named(session: &str) -> bool {
    !session.is_empty() && session.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

impl FinishRecords {
    pub fn here() -> Self {
        let variables: Vec<(String, String)> = [STORE_ENV, "XDG_STATE_HOME"].into_iter().filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?))).collect();
        Self::of(&variables, &PathBuf::from(std::env::var_os("HOME").unwrap_or_default()))
    }

    /// The records of a process whose environment is `environment` and whose home is `home`, as `here` reads them from its own.
    pub fn of(environment: &[(String, String)], home: &Path) -> Self {
        Self(verdict_store(environment, home).join(DIR))
    }

    fn path(&self, session: &str) -> Option<PathBuf> {
        named(session).then(|| self.0.join(format!("{session}.json")))
    }

    pub fn read(&self, session: &str) -> Option<Finished> {
        parse(session, &fs::read_to_string(self.path(session)?).ok()?)
    }

    /// Every finish record the store holds that reads as the record of the session it is named for.
    pub fn all(&self) -> Vec<Finished> {
        let names = fs::read_dir(&self.0).into_iter().flatten().flatten();
        names.filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let session = name.strip_suffix(".json")?;
            parse(session, &fs::read_to_string(entry.path()).ok()?)
        }).collect()
    }

    pub fn record(&self, finished: &Finished) -> io::Result<()> {
        let path = self.path(&finished.session).ok_or_else(|| io::Error::other(format!("session {:?} names no finish record", finished.session)))?;
        fs::create_dir_all(&self.0)?;
        let pending = tempfile::NamedTempFile::new_in(&self.0)?;
        fs::write(pending.path(), serde_json::to_string(finished).map_err(io::Error::other)?)?;
        pending.persist(&path).map(|_| ()).map_err(|error| error.error)
    }
}

/// The verdict store of a process whose environment is `environment` and whose home is `home`: the directory no worker domain writes, which holds the finish records and the reviewers' holds on commits.
pub fn verdict_store(environment: &[(String, String)], home: &Path) -> PathBuf {
    let set = |name: &str| environment.iter().find(|(own, value)| own == name && !value.is_empty()).map(|(_, value)| PathBuf::from(value));
    set(STORE_ENV).unwrap_or_else(|| set("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")).join("north/verdict-cache"))
}

pub fn parse(session: &str, text: &str) -> Option<Finished> {
    serde_json::from_str::<Finished>(text).ok().filter(|finished| finished.session == session)
}

pub fn remote_read(session: &str) -> Option<String> {
    named(session).then(|| format!("cat {REMOTE_DIR}/{DIR}/{session}.json"))
}

/// Whether a finish call runs a portable acceptance command as one more check of its own run before queueing, read from `finish_acceptance_in_prequeue`; only `off` turns it off.
pub fn acceptance_in_prequeue() -> bool {
    document(DEFAULTS)[SWITCH]["value"].as_str() != Some("off")
}
