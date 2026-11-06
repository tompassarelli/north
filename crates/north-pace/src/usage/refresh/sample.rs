//! `north usage sample`: the one-minute reader. Token accounts are read from their Messages response headers
//! (utilization, reset and status), every account with a login from `claude -p /usage` (which carries the
//! `extra_usage` flag), and the readings are recorded in the usage table.
use rusqlite::Connection;
use serde_json::Value;

use super::claude;
use super::record::{self, Reading};
use crate::usage::{TokenHeaders, events};

pub(super) struct Ports<'a> {
    pub headers: &'a dyn Fn() -> TokenHeaders,
    pub cli: &'a (dyn Fn() -> Vec<Reading> + Sync),
    pub ingest: &'a dyn Fn(&[u8]) -> Result<(), String>,
    pub at: f64,
}

pub(super) fn run(db: &Connection, mirrored: bool, ports: &Ports) -> Result<usize, String> {
    let rows = std::thread::scope(|scope| {
        let cli = scope.spawn(ports.cli);
        let mut rows = token_readings(&(ports.headers)(), ports.at);
        rows.extend(cli.join().unwrap_or_default());
        rows
    });
    record::publish(db, mirrored, &rows, ports.ingest)?;
    Ok(rows.len())
}

fn token_readings(headers: &[(String, Result<String, String>)], at: f64) -> Vec<Reading> {
    headers
        .iter()
        .flat_map(|(label, headers)| {
            headers.clone().and_then(|headers| events::header_rows(label, &headers, at)).unwrap_or_else(|error| {
                eprintln!("north usage sample: claude:{label} header read failed: {error}");
                Vec::new()
            })
        })
        .filter_map(|row| match row {
            Value::Object(row) => Some(row),
            _ => None,
        })
        .collect()
}

pub(super) fn cli_readings() -> Vec<Reading> {
    claude::read(&claude::Settings::from_env(true))
}
