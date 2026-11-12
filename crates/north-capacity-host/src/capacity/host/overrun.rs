//! The lines a running lease sends when it outlasts what its signature usually takes: one to the session that started it past `lease_overrun_session_multiple` times its usual wall time, one to that session's parent past `lease_overrun_parent_multiple`, and nothing is killed.

use std::time::Duration;

use serde_json::Value;

use super::json::text;
use super::shape::command_shape;
use crate::capacity::admission::default_mib;

const SENDER: &str = "service:north-capacity-lease";
const SEND_WAIT: Duration = Duration::from_secs(30);
const SHOWN_COMMAND: usize = 80;

/// What the line measures the run against: the signature's median wall time, or the declared deadline when the signature has too few runs to have one.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Usual {
    Measured { seconds: f64, runs: u64 },
    Thin { runs: u64, deadline: u64 },
}

/// Who a line goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reader {
    Session,
    Parent,
}

/// The two lines one lease may send, and how far it has got.
#[derive(Clone, Debug)]
pub(super) struct Overrun {
    session: String,
    what: String,
    usual: Usual,
    sent: usize,
}

impl Usual {
    /// The usual wall time from the admission forecast, else the declared deadline, else nothing to measure against.
    pub(super) fn of(forecast: Option<&Value>, timeout_seconds: Option<u64>) -> Option<Self> {
        let runs = forecast.and_then(|forecast| forecast.get("runs")).and_then(Value::as_u64).unwrap_or(0);
        let known = forecast.and_then(|forecast| text(forecast, "basis")).is_some_and(|basis| basis != "unknown");
        let seconds = forecast.and_then(|forecast| forecast.get("wallSeconds")).and_then(Value::as_f64).filter(|seconds| seconds.is_finite() && *seconds > 0.0);
        match seconds.filter(|_| known && runs as f64 >= default_mib("lease_overrun_minimum_runs")) {
            Some(seconds) => Some(Usual::Measured { seconds, runs }),
            None => timeout_seconds.filter(|deadline| *deadline > 0).map(|deadline| Usual::Thin { runs, deadline }),
        }
    }

    fn at(&self, reader: Reader) -> Duration {
        let floor = default_mib("lease_overrun_floor_seconds");
        let (session, parent) = (default_mib("lease_overrun_session_multiple"), default_mib("lease_overrun_parent_multiple"));
        let seconds = match (self, reader) {
            (Usual::Measured { seconds, .. }, Reader::Session) => (seconds * session).max(floor),
            (Usual::Measured { seconds, .. }, Reader::Parent) => (seconds * session).max(floor) * parent / session,
            (Usual::Thin { deadline, .. }, Reader::Session) => *deadline as f64 * default_mib("lease_overrun_thin_session_share"),
            (Usual::Thin { deadline, .. }, Reader::Parent) => *deadline as f64 * default_mib("lease_overrun_thin_parent_share"),
        };
        Duration::from_secs_f64(seconds)
    }

    fn told(&self, reader: Reader) -> String {
        match (self, reader) {
            (Usual::Measured { seconds, runs }, _) => {
                let multiple = default_mib(if reader == Reader::Session { "lease_overrun_session_multiple" } else { "lease_overrun_parent_multiple" });
                format!("its signature usually takes {} (median of {runs} runs), and this line comes at {multiple}x that", minutes(*seconds))
            }
            (Usual::Thin { runs, deadline }, _) => {
                let share = default_mib(if reader == Reader::Session { "lease_overrun_thin_session_share" } else { "lease_overrun_thin_parent_share" });
                format!("its signature has too few runs to say what is usual ({runs} of {}), so this line comes at {}% of its declared deadline of {}", default_mib("lease_overrun_minimum_runs"), (share * 100.0).round(), minutes(*deadline as f64))
            }
        }
    }
}

impl Overrun {
    pub(super) fn watch(session: Option<String>, command: &[String], usual: Option<Usual>) -> Option<Self> {
        let session = session.filter(|session| !session.trim().is_empty())?;
        let shape = command_shape(command);
        let what = match shape.char_indices().nth(SHOWN_COMMAND) {
            Some((cut, _)) => format!("{}…", &shape[..cut]),
            None => shape,
        };
        Some(Self { session, what, usual: usual?, sent: 0 })
    }

    fn reader(&self) -> Option<Reader> {
        [Reader::Session, Reader::Parent].get(self.sent).copied()
    }

    /// How long after the lease began the next line is due, or nothing once both have gone.
    pub(super) fn due(&self) -> Option<Duration> {
        self.reader().map(|reader| self.usual.at(reader))
    }

    /// The next line for `elapsed` of running, and who reads it; marks it sent.
    pub(super) fn next(&mut self, elapsed: Duration) -> Option<(Reader, String)> {
        let reader = self.reader()?;
        self.sent += 1;
        let ran = minutes(elapsed.as_secs_f64());
        let text = match reader {
            Reader::Session => format!("lease overrun: `{}` has run {ran}; {}. Nothing is killed; end it yourself if it is stuck.", self.what, self.usual.told(reader)),
            Reader::Parent => format!("lease overrun: {}'s `{}` has run {ran}; {}. Nothing is killed; it was told at the first line.", self.session, self.what, self.usual.told(reader)),
        };
        Some((reader, text))
    }

    pub(super) fn session(&self) -> &str {
        &self.session
    }
}

fn minutes(seconds: f64) -> String {
    if seconds < 120.0 { format!("{} s", seconds.round()) } else { format!("{} min", (seconds / 60.0).round()) }
}

/// The parent of a live fleet session, from the fleet's own list.
async fn parent_of(north: &str, session: &str) -> Option<String> {
    let mut list = tokio::process::Command::new(north);
    list.args(["fleet", "list", "--json", "--all"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(SEND_WAIT, list.output()).await.ok()?.ok().filter(|output| output.status.success())?;
    let rows: Value = serde_json::from_slice(&output.stdout).ok()?;
    rows.as_array()?.iter().find(|row| text(row, "name") == Some(session) && row.get("past").and_then(Value::as_bool) != Some(true)).and_then(|row| text(row, "parent")).map(str::to_owned)
}

/// Sends one line to its reader, bounded by `SEND_WAIT`; a line that cannot go is said on stderr.
pub(super) async fn deliver(north: String, session: String, reader: Reader, line: String) {
    let to = match reader {
        Reader::Session => Some(session),
        Reader::Parent => parent_of(&north, &session).await,
    };
    let Some(to) = to else {
        eprintln!("machine-capacity: {line} (no parent to tell)");
        return;
    };
    let mut send = tokio::process::Command::new(&north);
    send.args(["fleet", "send", "--now", "--from", SENDER, &to, &line]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).kill_on_drop(true);
    match tokio::time::timeout(SEND_WAIT, send.status()).await {
        Ok(Ok(status)) if status.success() => {}
        _ => eprintln!("machine-capacity: could not tell {to}: {line}"),
    }
}
