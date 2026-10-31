//! The note a parent gets once when a child stops with nothing to show for its last turn: no final
//! report, or a `Done:` report while its lane still holds commits origin/main lacks. It goes out
//! when the child has sat idle for `SILENT_AFTER`, or when it parks or exits first.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use super::receipts::is_final;
use super::stranded::{Lane, reported_done};

/// How long a child may sit idle before its parent is told it stopped without a report.
pub const SILENT_AFTER: Duration = Duration::from_secs(4 * 60);

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
}

impl Quiet {
    pub fn after(turn: u64, reply: String) -> Self {
        Self { turn, reply, checked: false, idled: false, probe: None, cancelled: Arc::default() }
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
pub fn notice(child: &str, state: State, turn: u64, reply: &str, lane: Option<&Lane>, unread: bool) -> Option<String> {
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
    Some(format!("[from {child} · {} · turn {turn}] {what}. Read it with `north fleet show {child}`.", state.label()))
}

/// A `Done: queued BRANCH` report, made while the branch waits for Autoland to land it.
fn queued(reply: &str) -> bool {
    let opening = reply.trim_start_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase();
    opening.strip_prefix("done:").is_some_and(|rest| rest.trim_start().starts_with("queued"))
}

fn commits(count: u32) -> String {
    format!("{count} commit{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn note(child: &str, state: State, turn: u64, reply: &str, lane: Option<&Lane>) -> Option<String> {
        notice(child, state, turn, reply, lane, false)
    }

    fn lane(ahead: u32, landed: bool) -> Lane {
        Lane { path: PathBuf::from("/code/north/worktrees/g1"), branch: Some("g1".into()), ahead, dirty: false, landed }
    }

    #[test]
    fn a_turn_without_a_final_report_is_named_with_the_state_and_the_ahead_count_of_its_lane() {
        let said = note("kid", State::Parked, 3, "Landing now.", Some(&lane(2, false))).unwrap();
        assert_eq!(said, "[from kid · parked · turn 3] its last turn ended with no Done:, Not done:, Split:, Stop: or Blocked: report; its lane g1 holds 2 commits not on origin/main. Read it with `north fleet show kid`.");
        assert!(note("kid", State::Idle, 3, "Landing now.", None).unwrap().contains("[from kid · idle · turn 3] its last turn ended with no"));
    }

    #[test]
    fn dropping_a_quiet_turn_cancels_the_check_still_running_for_it() {
        let quiet = Quiet::after(1, "x".into());
        let flag = quiet.cancelled.clone();
        assert!(!flag.load(Ordering::SeqCst));
        drop(quiet);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn a_lane_that_could_not_be_read_is_said_so_for_a_turn_with_no_report() {
        let said = notice("kid", State::Idle, 2, "Landing now.", None, true).unwrap();
        assert!(said.contains("report; its lane could not be read."), "{said}");
        assert_eq!(notice("kid", State::Idle, 2, "Done: x", None, true), None, "a Done: with an unread lane is left to the retry");
    }

    #[test]
    fn a_done_report_is_told_only_while_its_lane_holds_unlanded_commits() {
        let said = note("kid", State::Exited, 4, "Done: landed.", Some(&lane(1, false))).unwrap();
        assert!(said.contains("exited") && said.contains("it reported Done: but its lane g1 holds 1 commit not on origin/main"), "{said}");
        assert_eq!(note("kid", State::Exited, 4, "Done: landed.", Some(&lane(0, false))), None);
        assert_eq!(note("kid", State::Exited, 4, "Done: landed.", Some(&lane(3, true))), None, "its change landed in another form");
        assert_eq!(note("kid", State::Exited, 4, "Done: landed.", None), None);
        assert_eq!(note("kid", State::Exited, 4, "Done: queued claude/land-9", Some(&lane(1, false))), None, "a queued branch is not yet landed by design");
    }

    #[test]
    fn other_final_reports_stand_alone_and_need_no_lane_read() {
        for reply in ["Not done: red\nStop: landing", "Split: two parts", "Blocked: sign-in", "Stop: decision"] {
            assert_eq!(note("kid", State::Parked, 1, reply, Some(&lane(5, false))), None, "{reply}");
            assert!(!needs_lane(reply), "{reply}");
        }
        assert!(needs_lane("Done: x") && needs_lane("working on it"));
    }
}
