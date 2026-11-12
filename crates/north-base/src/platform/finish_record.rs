//! The finish record (north#1233): the verdict a session's last finish call got for its acceptance command. The
//! finish call writes it, and the session watcher reads a failed acceptance check from it, never from the lane or from
//! the text of a tool result or a report.
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const STORE_ENV: &str = "NORTH_VERDICT_CACHE_DIR";
const DIR: &str = "finished";
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
        let store = std::env::var_os(STORE_ENV).map(PathBuf::from).unwrap_or_else(|| {
            let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
            state.join("north/verdict-cache")
        });
        Self(store.join(DIR))
    }

    fn path(&self, session: &str) -> Option<PathBuf> {
        named(session).then(|| self.0.join(format!("{session}.json")))
    }

    pub fn read(&self, session: &str) -> Option<Finished> {
        parse(session, &fs::read_to_string(self.path(session)?).ok()?)
    }

    pub fn record(&self, finished: &Finished) -> io::Result<()> {
        let path = self.path(&finished.session).ok_or_else(|| io::Error::other(format!("session {:?} names no finish record", finished.session)))?;
        fs::create_dir_all(&self.0)?;
        let pending = tempfile::NamedTempFile::new_in(&self.0)?;
        fs::write(pending.path(), serde_json::to_string(finished).map_err(io::Error::other)?)?;
        pending.persist(&path).map(|_| ()).map_err(|error| error.error)
    }
}

pub fn parse(session: &str, text: &str) -> Option<Finished> {
    serde_json::from_str::<Finished>(text).ok().filter(|finished| finished.session == session)
}

pub fn remote_read(session: &str) -> Option<String> {
    named(session).then(|| format!("cat {REMOTE_DIR}/{DIR}/{session}.json"))
}
