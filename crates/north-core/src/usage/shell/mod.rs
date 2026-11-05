//! The usage shell: gathers a recorded `World` from the usage table, the fleet registry and the Agent-tool
//! launch log, takes the decision lock, runs the pure `place`, writes the launch record and logs the decision.
mod assemble;
mod brief;
mod fleet_records;
mod launches;
mod lock;
mod log;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::decide::world::{Label, Request, Session, Unix};
use super::decide::{Decision, explain, place};
use crate::foundation::clock::Timestamp;

pub use assemble::Parts;
pub use fleet_records::FleetRecords;
pub use launches::{Launch, LaunchLog};

pub type Record<'a> = dyn Fn(&Label) -> Result<(), String> + 'a;

pub trait Registry {
    fn sessions(&self) -> Result<Vec<Session>, String>;
}

pub struct Shell<'a> {
    pub db: &'a Connection,
    pub policy: &'a Path,
    pub host: &'a str,
    pub registry: &'a dyn Registry,
    pub directory: PathBuf,
}

pub struct Outcome {
    pub decision: Decision,
    pub line: String,
    pub in_flight: Vec<String>,
}

impl Shell<'_> {
    pub fn launch_log(&self) -> LaunchLog {
        LaunchLog::at(&self.directory.join("launches.jsonl"))
    }

    pub fn world(&self, now: Unix) -> Result<Parts, String> {
        let mut sessions = self.registry.sessions()?;
        sessions.extend(self.launch_log().read().iter().filter_map(Launch::session));
        assemble::parts(self.db, self.policy, self.host, now, sessions)
    }

    pub fn decide(
        &self,
        now: Unix,
        request: &Request,
        record: &Record<'_>,
    ) -> Result<Outcome, String> {
        let _lock = lock::Lock::take(&self.directory.join("decide.lock"))?;
        self.judge(now, request, Some(record))
    }

    pub fn dry_run(&self, now: Unix, request: &Request) -> Result<Outcome, String> {
        let _lock = lock::Lock::take(&self.directory.join("decide.lock"))?;
        self.judge(now, request, None)
    }

    fn judge(
        &self,
        now: Unix,
        request: &Request,
        record: Option<&Record<'_>>,
    ) -> Result<Outcome, String> {
        let parts = self.world(now)?;
        let world = parts.world();
        let decision = place(&world, request);
        let line = explain(&decision, &local_time);
        let in_flight: Vec<String> = parts
            .sessions
            .iter()
            .map(|session| session.name.0.clone())
            .collect();
        if let Some(record) = record {
            if let Decision::Place { account, .. } = &decision {
                record(account)?;
            }
            log::append(
                &self.directory.join("decisions.jsonl"),
                &log::Entry {
                    at: now,
                    request,
                    world: &world,
                    in_flight: &in_flight,
                    line: &line,
                },
            )?;
        }
        Ok(Outcome {
            decision,
            line,
            in_flight,
        })
    }
}

fn local_time(at: Unix) -> String {
    Timestamp::from_unix_seconds(at.0).local_twelve_hour()
}

pub fn dry_run(shell: &Shell, brief: &str) -> Result<String, String> {
    let now = Unix(super::clock().unix_seconds());
    let outcome = shell.dry_run(now, &brief::request(brief, shell.host))?;
    let flying = if outcome.in_flight.is_empty() {
        "none".to_owned()
    } else {
        outcome.in_flight.join(", ")
    };
    Ok(format!("{}\nin flight: {flying}", outcome.line))
}
