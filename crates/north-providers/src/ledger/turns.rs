//! The worker turns a window started, one row per run as the threads ledger holds it, whichever provider ran it: the
//! tokens in and out, the wall minutes, the outcome and the sha its first hand-in landed as (north#1589).
use std::path::Path;
use std::time::Duration;

use super::closed_source::window_bounds;
use super::threads_database;
use north_foundation::error::NorthResult;
use north_foundation::foundation::clock::Timestamp;
use north_base::platform::sqlite::{self, Access};

#[derive(Clone, Debug, PartialEq)]
pub struct WorkerTurn {
    pub agent: String,
    pub provider: String,
    pub fleet: String,
    pub started: String,
    pub minutes: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub outcome: String,
    pub landed: bool,
    pub landed_sha: String,
}

/// The worker turns that started in the last `window`, read from the threads ledger.
pub fn worker_turns(window: &str) -> NorthResult<Vec<WorkerTurn>> {
    worker_turns_in(&threads_database(), window, Timestamp::now())
}

pub fn worker_turns_in(database: &Path, window: &str, now: Timestamp) -> NorthResult<Vec<WorkerTurn>> {
    let (since, until) = window_bounds(window, now)?;
    let rows = sqlite::rows(
        database,
        Access::ReadOnly,
        Duration::from_secs(5),
        &format!(
            "SELECT agent, COALESCE(provider, '') AS provider, COALESCE(fleet_name, '') AS fleet, COALESCE(started, '') AS started, COALESCE(minutes, 0) AS minutes, \
             COALESCE(input_tokens, 0) AS input, COALESCE(tokens, 0) AS output, COALESCE(outcome, '') AS outcome, COALESCE(landed, 0) AS landed, COALESCE(landed_sha, '') AS landed_sha \
             FROM runs WHERE role = 'worker' AND started >= '{}' AND started < '{}' ORDER BY started",
            since.replace('\'', ""),
            until.replace('\'', "")
        ),
    )?;
    rows.iter()
        .map(|row| {
            let text = |key: &str| row[key].as_str().unwrap_or_default().to_owned();
            let landed_sha = text("landed_sha");
            Ok(WorkerTurn {
                agent: text("agent"),
                provider: text("provider"),
                fleet: text("fleet"),
                started: text("started"),
                minutes: row["minutes"].as_i64().unwrap_or(0),
                tokens_in: row["input"].as_i64().unwrap_or(0),
                tokens_out: row["output"].as_i64().unwrap_or(0),
                outcome: text("outcome"),
                landed: row["landed"].as_i64() == Some(1) || !landed_sha.is_empty(),
                landed_sha,
            })
        })
        .collect()
}
