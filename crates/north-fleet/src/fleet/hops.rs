//! A launch's trace: one span per hop from the launch command's start to the model's first answer,
//! appended to `hops.jsonl` in the session's directory by the process that ends the hop. The launch
//! command writes its own span to registration and one for each of its steps ([`COMMAND`]); the registry ends the
//! host-open hop when the session host it opens has started, and the session-host hop over it when the
//! host goes live, so the gap between them is the launch waiting on its name's start lock and its record; `north session-link` ends the remote-unit hop when the
//! other machine answers that the CLI's unit started, and the client hop at the CLI's first output;
//! the first turn ends the prompt hop at the client's first user line, where it takes the prompt once its
//! startup and session hooks are done and only then sends its request, and the response hop, that
//! request, at the first answer. Every start appends a session-host span, so each start reads as its own
//! launch.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::runtime::now_ms;

const FILE: &str = "hops.jsonl";
const ASKED_WITHIN_MS: u64 = 10 * 60 * 1000;
const USER_HZ: f64 = 100.0;
/// The launch command's own hops, in the order they run between its start and registration.
pub const COMMAND: [Hop; 5] = [Hop::Startup, Hop::Admission, Hop::Placement, Hop::Gating, Hop::Prelude];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Hop {
    Launch,
    Startup,
    Admission,
    Placement,
    Gating,
    Prelude,
    HostOpen,
    SessionHost,
    RemoteUnit,
    Client,
    Prompt,
    Response,
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
    pub host_open_ms: Option<u64>,
    pub session_host_ms: u64,
    pub asked_ms: Option<u64>,
    pub command: Vec<(Hop, u64)>,
    pub remote_unit_ms: Option<u64>,
    pub client_ms: Option<u64>,
    pub prompt_ms: Option<u64>,
    pub response_ms: Option<u64>,
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

/// Writes a launch command's own hops to `trace`: from its process's start to registration at `registered_ms`, and each step of [`COMMAND`] within that, which `marks` (entering the launch, placing, placed, launching) bound.
pub fn record_command(trace: &Path, session: &str, registered_ms: u64, marks: [u64; 4]) {
    let span = |hop, start_ms, end_ms| Span { session: session.to_owned(), hop, start_ms, end_ms };
    let asked_ms = process_started_ms();
    if let Some(asked_ms) = asked_ms {
        record(trace, &span(Hop::Launch, asked_ms, registered_ms));
    }
    let bounds = [asked_ms, Some(marks[0]), Some(marks[1]), Some(marks[2]), Some(marks[3]), Some(registered_ms)];
    for (hop, pair) in COMMAND.into_iter().zip(bounds.windows(2)) {
        if let [Some(start_ms), Some(end_ms)] = *pair {
            record(trace, &span(hop, start_ms, end_ms));
        }
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
        launch.host_open_ms = ended(Hop::HostOpen);
        launch.remote_unit_ms = ended(Hop::RemoteUnit);
        launch.client_ms = ended(Hop::Client);
        launch.prompt_ms = ended(Hop::Prompt);
        launch.response_ms = ended(Hop::Response);
        let before = |hop: Hop| spans.iter().find(|span| span.hop == hop && span.end_ms <= launch.registered_ms && launch.registered_ms - span.end_ms < ASKED_WITHIN_MS);
        launch.asked_ms = before(Hop::Launch).map(|span| span.start_ms);
        launch.command = COMMAND.iter().filter_map(|&hop| before(hop).map(|span| (hop, span.end_ms.saturating_sub(span.start_ms)))).collect();
    }
    starts
}

/// When this process started, from its start tick and the clock's uptime, so a launch command's span begins where its caller's clock does.
pub fn process_started_ms() -> Option<u64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let ticks: f64 = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()?;
    let uptime: f64 = fs::read_to_string("/proc/uptime").ok()?.split_whitespace().next()?.parse().ok()?;
    let since_ms = ((uptime - ticks / USER_HZ) * 1000.0).max(0.0);
    Some(now_ms().saturating_sub(since_ms as u64))
}
