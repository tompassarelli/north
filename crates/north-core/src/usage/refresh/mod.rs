//! `north usage --refresh`: read every account's adapter (and the labelled accounts signed in on other machines),
//! record the readings in the usage table (a remote client publishes them to the host first), rewrite the pace gate
//! and notify promotions that are about to end.
use std::fs::OpenOptions;
use std::path::Path;

use rusqlite::Connection;
use rustix::fs::{FlockOperation, flock};

use super::{Host, Places, gate, settings, store};
use adapters::Readers;
use crate::foundation::clock::Timestamp;

mod adapters;
mod promos;
mod record;
#[cfg(test)]
mod tests;

struct Ports<'a> {
    pub readers: &'a Readers<'a>,
    pub ingest: &'a dyn Fn(&[u8]) -> Result<(), String>,
    pub notify: &'a dyn Fn(&str, &str),
    pub clock: &'a dyn Fn() -> Timestamp,
}

struct Request {
    pub mirrored: bool,
    pub remote: bool,
}

fn run(db: &Connection, places: &Places, request: &Request, ports: &Ports) -> Result<(), String> {
    record::publish(db, request.mirrored, &ports.readers.read_all(request.remote), ports.ingest)?;
    let now = (ports.clock)();
    let mut providers: Vec<String> = super::readings(db, &places.policy)?.iter().filter_map(|row| row.text("provider").map(str::to_owned)).collect();
    providers.sort();
    providers.dedup();
    let named: Vec<&str> = providers.iter().map(String::as_str).collect();
    {
        let _control = hold_pace_control(&places.state.join("agents"))?;
        gate::write(db, &named, places, now)?;
    }
    promos::notify(&promos::lines(&settings::catalog(), db, now)?, &places.state, ports.notify)
}

fn notify_send(title: &str, line: &str) {
    let _ = std::process::Command::new("notify-send").args([title, line]).status();
}

fn hold_pace_control(directory: &Path) -> Result<std::fs::File, String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let file = OpenOptions::new().create(true).append(true).open(directory.join("pace-control.lock")).map_err(|error| error.to_string())?;
    flock(&file, FlockOperation::LockExclusive).map_err(|error| error.to_string())?;
    Ok(file)
}

pub(super) fn refresh_readings(db: &Connection, source: &store::Source, places: &Places, remote: bool, host: &Host) -> Result<(), String> {
    let readers = Readers::new(host, (host.harness_lib)()?);
    let ports = Ports { readers: &readers, ingest: host.ingest, notify: &notify_send, clock: &super::clock };
    run(db, places, &Request { mirrored: source.mirrored(), remote }, &ports)
}

pub(super) fn read_provider(db: &Connection, source: &store::Source, provider: &str, host: &Host) -> Result<(), String> {
    let readers = Readers::new(host, (host.harness_lib)()?);
    let rows: Vec<_> = readers.read_all(false).into_iter().filter(|row| row.get("provider").and_then(serde_json::Value::as_str) == Some(provider)).collect();
    record::publish(db, source.mirrored(), &rows, host.ingest)
}
