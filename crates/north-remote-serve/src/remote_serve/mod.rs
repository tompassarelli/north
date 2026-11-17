pub mod audit;
mod env;
mod lexer;
pub mod log;
pub mod south_land;
pub mod south_policy;
mod verbs;
mod worker_verbs;

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::time::Instant;

use north_foundation::foundation::clock::Timestamp;

pub const NEXUS_PEER: &str = "nexus";
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
    Verb(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(out, "empty command"),
            Self::TooLong => write!(out, "command longer than {MAX_COMMAND_BYTES} bytes"),
            Self::UnterminatedQuote => write!(out, "unterminated quote"),
            Self::Metacharacter(c) => write!(out, "shell metacharacter {c:?}"),
            Self::EnvKey(key) => write!(out, "environment key {key} is not allowed"),
            Self::Verb(verb) => write!(out, "command {verb} is not on the allowlist"),
        }
    }
}

pub fn plan(original_command: &str) -> Result<Plan, Refusal> {
    plan_with(original_command, verbs::authorize)
}

fn plan_with(original_command: &str, authorize: fn(&[String]) -> Result<String, Refusal>) -> Result<Plan, Refusal> {
    if original_command.len() > MAX_COMMAND_BYTES {
        return Err(Refusal::TooLong);
    }
    let words = lexer::split(original_command)?;
    let argv = env::strip_prefix(&words)?;
    if argv.is_empty() {
        return Err(Refusal::Empty);
    }
    let verb = authorize(argv)?;
    Ok(Plan { verb, argv: argv.to_vec(), env: BTreeMap::new() })
}

pub struct Grants {
    pub peer: String,
}

pub trait Launch {
    fn launch(&self, plan: &Plan) -> i32;
}

pub fn peer_name(peer: &str) -> bool {
    (1..=64).contains(&peer.len()) && peer.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn plan_for(original_command: &str, grants: &Grants) -> Result<Plan, Refusal> {
    if grants.peer == NEXUS_PEER {
        plan_with(original_command, worker_verbs::authorize)
    } else {
        plan(original_command)
    }
}

pub fn serve(original_command: Option<&str>, grants: &Grants, launch: &dyn Launch, log: &mut log::Log, err: &mut dyn Write) -> io::Result<i32> {
    let (ts, started) = (Timestamp::now(), Instant::now());
    let command = original_command.unwrap_or("");
    let (verb, refusal, exit) = match plan_for(command, grants) {
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
