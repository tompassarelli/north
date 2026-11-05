//! `north cloud reap`: lands the finished patch handback of every session `north cloud launch`
//! started, through the same landing as `north cloud wait`, so no one has to wait on a worker by
//! hand. A user timer runs it every two minutes; each run is idempotent.
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::cloud::{Api, field, request, save, turn_ended};
use super::cloud_land::Outcome;
use crate::dispatch::CloudLaunch;

/// A launch older than this is no longer reaped: its session has long finished or been swept.
const WINDOW_S: f64 = 7.0 * 24.0 * 3600.0;

/// Lands one finished session's transcript, `None` when it holds no handback.
pub(super) type Land<'a> = dyn FnMut(&CloudLaunch, &[Value], &Path) -> Option<Outcome> + 'a;

/// What one reap run did, so the caller can exit nonzero when a landing stopped or a session
/// could not be read.
#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct Reaped {
    pub(super) landed: usize,
    pub(super) failed: usize,
}

/// Per-session files under `state`: `SESSION.landed` holds the report of a queued handback, so the
/// session is never read again; `SESSION.seen` holds the `updated_at` of a finished turn that had
/// no handback, so an unchanged session is not reread; `SESSION.jsonl` is its saved transcript.
fn marker(state: &Path, session: &str, kind: &str) -> PathBuf {
    state.join(format!("{session}.{kind}"))
}

fn record(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, format!("{text}\n")).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn reap(launches: &[CloudLaunch], api: &mut Api, state: &Path, now: f64, land: &mut Land, out: &mut dyn Write) -> Result<Reaped, String> {
    std::fs::create_dir_all(state).map_err(|error| format!("{}: {error}", state.display()))?;
    let mut reaped = Reaped::default();
    let mut done = std::collections::BTreeSet::new();
    for launch in launches.iter().rev() {
        let id = launch.session.as_str();
        if now - launch.at > WINDOW_S || !done.insert(id) || marker(state, id, "landed").exists() {
            continue;
        }
        match reap_one(launch, api, state, land) {
            Ok(Step::Left(why)) => {
                let _ = writeln!(out, "{id} ({} {}): left alone, {why}", launch.repo, launch.branch);
            }
            Ok(Step::Unchanged) => {}
            Ok(Step::Landed(text)) => {
                reaped.landed += 1;
                let _ = writeln!(out, "{id} ({} {}): {text}", launch.repo, launch.branch);
            }
            Ok(Step::Stopped(text)) | Err(text) => {
                reaped.failed += 1;
                let _ = writeln!(out, "{id} ({} {}): {text}", launch.repo, launch.branch);
            }
        }
    }
    Ok(reaped)
}

enum Step {
    Left(String),
    Unchanged,
    Landed(String),
    Stopped(String),
}

fn reap_one(launch: &CloudLaunch, api: &mut Api, state: &Path, land: &mut Land) -> Result<Step, String> {
    let id = launch.session.as_str();
    let session = request(api, "GET", &format!("/v1/sessions/{id}?beta=true"), None)?;
    let status = field(&session, "session_status");
    if status != "idle" {
        return Ok(Step::Left(format!("session {}", if status.is_empty() { "status unknown" } else { status })));
    }
    let updated = field(&session, "updated_at");
    let seen = marker(state, id, "seen");
    if !updated.is_empty() && std::fs::read_to_string(&seen).is_ok_and(|text| text.trim() == updated) {
        return Ok(Step::Unchanged);
    }
    let events = super::cloud::events(api, id)?;
    if !turn_ended(&events, None) {
        return Ok(Step::Left("its turn has not ended".into()));
    }
    let transcript = marker(state, id, "jsonl");
    save(&events, &transcript)?;
    let Some(outcome) = land(launch, &events, &transcript) else {
        record(&seen, updated)?;
        return Ok(Step::Left("finished with no patch handback".into()));
    };
    let (_, text) = outcome.report();
    match outcome {
        Outcome::Queued { .. } | Outcome::Already(_) => {
            record(&marker(state, id, "landed"), &text)?;
            Ok(Step::Landed(text))
        }
        Outcome::Stopped { .. } => Ok(Step::Stopped(text)),
    }
}

/// The checkout a launch's repository lands from on this host: `~/code/REPO/main`.
fn checkout(repo: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code").join(crate::threads::catalog::checkout(repo)).join("main")
}

pub(super) fn run(api: &mut Api, out: &mut dyn Write) -> Result<u8, String> {
    let state = super::cloud_land::state_dir();
    let launches = crate::dispatch::cloud_launches();
    let mut land = |launch: &CloudLaunch, events: &[Value], transcript: &Path| {
        super::cloud_land::land_handback(events, transcript, &checkout(&launch.repo), &state, &mut super::cloud_land::run_program)
    };
    let now = crate::pacing::placement::now();
    let reaped = reap(&launches, api, &state.join("reap"), now, &mut land, out)?;
    Ok(u8::from(reaped.failed > 0))
}

#[cfg(test)]
#[path = "cloud_reap_tests.rs"]
mod tests;
