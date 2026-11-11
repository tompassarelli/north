//! A launch's trace: one span per hop from registration to the client's first output line, appended
//! to `hops.jsonl` in the session's directory by the process that ends the hop. The registry ends
//! the session-host hop when the host goes live; `north session-link` ends the remote-unit hop when
//! the other machine answers that the CLI's unit started, and the client hop at the CLI's first
//! output. Every start appends a session-host span, so each start reads as its own launch.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "hops.jsonl";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Hop {
    SessionHost,
    RemoteUnit,
    Client,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub session: String,
    pub hop: Hop,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// One start of a session as its trace tells it: when it registered or was revived, and when each hop ended.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Launch {
    pub session: String,
    pub registered_ms: u64,
    pub session_host_ms: u64,
    pub remote_unit_ms: Option<u64>,
    pub client_ms: Option<u64>,
}

pub fn trace_in(session_dir: &Path) -> PathBuf {
    session_dir.join(FILE)
}

pub fn record(trace: &Path, span: &Span) {
    let appended = serde_json::to_string(span).map_err(std::io::Error::from).and_then(|line| OpenOptions::new().create(true).append(true).open(trace)?.write_all(format!("{line}\n").as_bytes()));
    if let Err(error) = appended {
        eprintln!("north fleet: {}'s launch trace {} took no span: {error}", span.session, trace.display());
    }
}

pub fn launches(trace: &Path) -> Vec<Launch> {
    let spans: Vec<Span> = fs::read_to_string(trace).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let mut starts: Vec<Launch> = spans
        .iter()
        .filter(|span| span.hop == Hop::SessionHost)
        .map(|span| Launch { session: span.session.clone(), registered_ms: span.start_ms, session_host_ms: span.end_ms, ..Launch::default() })
        .collect();
    starts.sort_by_key(|launch| launch.registered_ms);
    let bounds: Vec<u64> = starts.iter().skip(1).map(|launch| launch.registered_ms).chain([u64::MAX]).collect();
    for (launch, next) in starts.iter_mut().zip(bounds) {
        let ended = |hop: Hop| spans.iter().filter(|span| span.hop == hop && span.end_ms >= launch.registered_ms && span.end_ms < next).map(|span| span.end_ms).min();
        launch.remote_unit_ms = ended(Hop::RemoteUnit);
        launch.client_ms = ended(Hop::Client);
    }
    starts
}
