//! The note a parent gets once when a child stops with nothing to show for its last turn: no final
//! report, or a `Done:` report while its lane still holds commits origin/main lacks. It goes out
//! when the child has gone quiet for twice its brief's ETA, or when it parks or exits first.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::model::Entry;
use super::receipts::{is_final, last_lines};
use super::stranded::{Lane, reported_done};

/// The ETA a child is held to when its brief names none.
pub const DEFAULT_ETA_MIN: u64 = 45;

/// How many ETAs a child may stay quiet before its parent is told.
const QUIET_ETAS: u32 = 2;

/// The minutes of the `ETA:` line of a brief, such as `ETA: 20 min (uncalibrated)` or `ETA: 1.5 h`.
pub fn eta_minutes(brief: &str) -> Option<u64> {
    let rest = brief.lines().find_map(|line| line.trim().strip_prefix("ETA:"))?.trim_start();
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
    let amount: f64 = rest[..end].parse().ok()?;
    let hours = rest[end..].trim_start().to_ascii_lowercase().starts_with('h');
    let minutes = (amount * if hours { 60.0 } else { 1.0 }).round();
    (minutes >= 1.0).then_some(minutes as u64)
}

/// How long a child with this ETA (`DEFAULT_ETA_MIN` when none) may stay quiet, an ETA minute lasting `minute`.
pub fn quiet_after(eta_min: Option<u64>, minute: Duration) -> Duration {
    minute.saturating_mul(u32::try_from(eta_min.unwrap_or(DEFAULT_ETA_MIN)).unwrap_or(u32::MAX).saturating_mul(QUIET_ETAS))
}

/// How long the child has been quiet, for the note to its parent.
pub struct Silence<'a> {
    pub quiet: Duration,
    pub eta_min: Option<u64>,
    pub lines: &'a [Entry],
}

impl Silence<'_> {
    fn text(&self) -> String {
        let quiet = self.quiet.as_secs() / 60;
        let eta = self.eta_min.map_or_else(|| format!("{DEFAULT_ETA_MIN} min (its brief names none)"), |eta| format!("{eta} min"));
        format!(" Quiet for {quiet} min against its ETA of {eta}.")
    }
}

/// Where the child stood when it was found silent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Idle,
    Parked,
    Exited,
}

impl State {
    fn label(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Parked => "parked",
            State::Exited => "exited",
        }
    }
}

/// The last turn a child finished and how far the check on whether to tell its parent has got.
pub struct Quiet {
    pub turn: u64,
    pub reply: String,
    /// The check is settled: the parent was told or needs no word.
    pub checked: bool,
    /// The idle check has run once; park or exit may run it again.
    pub idled: bool,
    /// The check running now, which answers whether it settled.
    pub probe: Option<JoinHandle<bool>>,
    /// Set once the child starts another turn, so a check still running says nothing.
    pub cancelled: Arc<AtomicBool>,
    pub completed: bool,
    pub since: Instant,
    /// How long the child may stay quiet before the idle check tells its parent.
    pub after: Duration,
    pub eta_min: Option<u64>,
    pub lines: Vec<Entry>,
}

impl Quiet {
    pub fn after(turn: u64, reply: String, completed: bool, eta_min: Option<u64>, minute: Duration, lines: Vec<Entry>) -> Self {
        Self { turn, reply, checked: false, idled: false, probe: None, cancelled: Arc::default(), completed, since: Instant::now(), after: quiet_after(eta_min, minute), eta_min, lines }
    }

    /// Whether the child has been quiet past twice its ETA and has not been told about yet.
    pub fn due(&self, idle: Duration) -> bool {
        !self.checked && !self.idled && idle >= self.after
    }

    /// Takes the result of a check that has ended.
    pub fn settle(&mut self) {
        if self.probe.as_ref().is_some_and(JoinHandle::is_finished) {
            self.checked = self.probe.take().is_some_and(|probe| probe.join().unwrap_or(true));
        }
    }
}

impl Drop for Quiet {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

/// Whether `notice` needs the child's lane read: a final report that is not `Done:` stands alone.
pub fn needs_lane(reply: &str) -> bool {
    !is_final(reply) || reported_done(reply)
}

/// The note for the parent, or nothing when the child reported or holds nothing unlanded.
pub fn notice(child: &str, state: State, turn: u64, reply: &str, lane: Option<&Lane>, unread: bool, silence: Option<&Silence>) -> Option<String> {
    let held = lane.filter(|lane| lane.ahead > 0 && !lane.landed);
    let what = if !is_final(reply) {
        let lane = held.map(|lane| format!("; its lane {} holds {} not on origin/main", lane.label(), commits(lane.ahead))).unwrap_or_default();
        let lane = if unread { "; its lane could not be read".to_owned() } else { lane };
        format!("its last turn ended with no Done:, Not done:, Split:, Stop: or Blocked: report{lane}")
    } else if let Some(lane) = held.filter(|_| reported_done(reply) && !queued(reply)) {
        format!("it reported Done: but its lane {} holds {} not on origin/main", lane.label(), commits(lane.ahead))
    } else {
        return None;
    };
    let quiet = silence.map(Silence::text).unwrap_or_default();
    let tail = silence.filter(|silence| !silence.lines.is_empty()).map(|silence| format!("\n{}", last_lines(silence.lines))).unwrap_or_default();
    Some(format!("[from {child} · {} · turn {turn}] {what}.{quiet} Read it with `north fleet show {child}`.{tail}", state.label()))
}

/// A `Done: queued BRANCH` report, made while the branch waits for Autoland to land it.
fn queued(reply: &str) -> bool {
    let opening = reply.trim_start_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase();
    opening.strip_prefix("done:").is_some_and(|rest| rest.trim_start().starts_with("queued"))
}

fn commits(count: u32) -> String {
    format!("{count} commit{}", if count == 1 { "" } else { "s" })
}

