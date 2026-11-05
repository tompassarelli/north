pub mod audit;
mod env;
mod guards;
mod lexer;
pub mod log;
pub mod south_land;
pub mod south_policy;
mod verbs;

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::foundation::clock::Timestamp;

pub use guards::REMOTE_PEER;

pub const MAX_COMMAND_BYTES: usize = 80 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub verb: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Empty,
    TooLong,
    UnterminatedQuote,
    Metacharacter(char),
    EnvKey(String),
    EnvValue(String),
    EnvRepeated(String),
    Verb(String),
    Guard(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(out, "empty command"),
            Self::TooLong => write!(out, "command longer than {MAX_COMMAND_BYTES} bytes"),
            Self::UnterminatedQuote => write!(out, "unterminated quote"),
            Self::Metacharacter(c) => write!(out, "shell metacharacter {c:?}"),
            Self::EnvKey(key) => write!(out, "environment key {key} is not allowed"),
            Self::EnvValue(key) => write!(out, "environment value for {key} does not match its pattern"),
            Self::EnvRepeated(key) => write!(out, "environment key {key} given twice"),
            Self::Verb(verb) => write!(out, "command {verb} is not on the allowlist"),
            Self::Guard(why) => write!(out, "{why}"),
        }
    }
}

pub fn plan(original_command: &str) -> Result<Plan, Refusal> {
    if original_command.len() > MAX_COMMAND_BYTES {
        return Err(Refusal::TooLong);
    }
    let words = lexer::split(original_command)?;
    let (env, argv) = env::split_prefix(&words)?;
    if argv.is_empty() {
        return Err(Refusal::Empty);
    }
    let verb = verbs::authorize(argv)?;
    Ok(Plan { verb, argv: argv.to_vec(), env })
}

pub struct Grants {
    pub peer: String,
    pub repos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub placement: String,
    pub item: Option<String>,
}

pub trait Jobs {
    fn job(&self, id: i64) -> Option<Job>;
}

pub trait Launch {
    fn launch(&self, plan: &Plan) -> i32;
}

pub struct LedgerJobs(pub PathBuf);

impl Jobs for LedgerJobs {
    fn job(&self, id: i64) -> Option<Job> {
        let db = Connection::open_with_flags(&self.0, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        db.query_row("SELECT placement, item FROM job_queue WHERE id = ?1", [id], |row| Ok(Job { placement: row.get(0)?, item: row.get(1)? })).optional().ok()?
    }
}

pub fn peer_name(peer: &str) -> bool {
    (1..=64).contains(&peer.len()) && peer.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn ledger(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    env.get("THREADS_DB")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| env.get("HOME").map(|home| Path::new(home).join(".local/state/threads/threads.db")))
}

pub fn plan_for(original_command: &str, grants: &Grants, jobs: &dyn Jobs) -> Result<Plan, Refusal> {
    guards::guard(plan(original_command)?, grants, jobs)
}

pub fn serve(original_command: Option<&str>, grants: &Grants, jobs: &dyn Jobs, launch: &dyn Launch, log: &mut log::Log, err: &mut dyn Write) -> io::Result<i32> {
    let (ts, started) = (Timestamp::now(), Instant::now());
    let command = original_command.unwrap_or("");
    let (verb, refusal, exit) = match plan_for(command, grants, jobs) {
        Ok(plan) => (Some(plan.verb.clone()), None, launch.launch(&plan)),
        Err(refusal) => {
            let _ = writeln!(err, "remote-serve: refused for {}: {refusal}", grants.peer);
            (None, Some(refusal.to_string()), log::REFUSED_EXIT)
        }
    };
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    log.append(&log::Row { ts: ts.iso(), peer: grants.peer.clone(), command: log::clipped(command), verb, refusal, exit, ms })?;
    Ok(exit)
}

#[cfg(test)]
mod tests;
