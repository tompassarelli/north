use std::path::Path;

use crate::error::NorthResult;

use super::gates::GateRequest;
use super::model::{Delivery, Entry, SessionRecord};
use super::runtime::summarize;
use super::turns::TurnExit;

const REPORT_CHARS: usize = 4_000;
const LINE_CHARS: usize = 300;
const LAST_LINES: usize = 5;
const FINAL_OPENINGS: [&str; 5] = ["Done", "Not done", "Split", "Stop", "Blocked"];

pub trait ReceiptBook: Send + Sync {
    fn claim_receipt(&self, session: &str, key: &str, claimed_ms: u64) -> NorthResult<bool>;
}

pub trait Courier: Send + Sync {
    fn post(&self, parent: &str, text: String, delivery: Delivery);
    fn ask(&self, request: GateRequest);
    fn answer(&self, host_dir: &Path, frame: String);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    pub child: String,
    pub turn: u64,
    pub exit: TurnExit,
    pub landed: Option<String>,
    pub reply: Option<String>,
}

impl Report {
    pub fn notice(&self, lines: &[Entry]) -> Option<(String, Delivery)> {
        let reply = self.reply.as_deref().unwrap_or_default().trim();
        if reply.is_empty() {
            return None;
        }
        match self.exit {
            TurnExit::Completed if self.landed.is_some() || is_final(reply) => Some((self.receipt(), Delivery::Queued)),
            TurnExit::Completed => None,
            TurnExit::Failed => Some((format!("{} The turn failed. {}", self.header(), last_lines(lines)), Delivery::Now)),
            TurnExit::Interrupted | TurnExit::Killed => None,
        }
    }

    fn key(&self) -> String {
        format!("turn:{}", self.turn)
    }

    fn header(&self) -> String {
        let landed = self.landed.as_deref().map(|sha| format!(" · landed {sha}")).unwrap_or_default();
        format!("[from {} · turn {} · exit {}{landed}]", self.child, self.turn, self.exit.label())
    }

    fn receipt(&self) -> String {
        let reply = self.reply.as_deref().map(str::trim).unwrap_or_default();
        match reply.char_indices().nth(REPORT_CHARS) {
            Some((cut, _)) => format!("{} {}…\n[report truncated; read it all with `north fleet show {}`]", self.header(), &reply[..cut], self.child),
            None => format!("{} {reply}", self.header()),
        }
    }
}

pub fn deliver_report(book: &dyn ReceiptBook, courier: &dyn Courier, parent: &str, report: &Report, lines: &[Entry], now_ms: u64) -> NorthResult<bool> {
    let Some((text, delivery)) = report.notice(lines) else { return Ok(false) };
    let claimed = book.claim_receipt(&report.child, &report.key(), now_ms)?;
    if claimed {
        courier.post(parent, text, delivery);
    }
    Ok(claimed)
}

pub struct HostDeath<'a> {
    pub child: &'a SessionRecord,
    pub host_pid: u32,
    pub mid_task: bool,
    pub lines: &'a [Entry],
}

pub fn still_watched(child: Option<&SessionRecord>, host_pid: u32) -> bool {
    child.is_some_and(|child| !child.stopped && child.host_pid == Some(host_pid))
}

pub fn deliver_death(book: &dyn ReceiptBook, courier: &dyn Courier, death: &HostDeath, now_ms: u64) -> NorthResult<bool> {
    let child = death.child;
    let Some(parent) = child.parent.as_deref() else { return Ok(false) };
    let restarted = child.host_pid != Some(death.host_pid);
    if child.stopped || restarted || !death.mid_task {
        return Ok(false);
    }
    let claimed = book.claim_receipt(&child.name, &format!("host:{}", death.host_pid), now_ms)?;
    if claimed {
        let text = format!("[from {} · host {} exited mid-task] {}", child.name, death.host_pid, last_lines(death.lines));
        courier.post(parent, text, Delivery::Now);
    }
    Ok(claimed)
}

fn is_final(reply: &str) -> bool {
    let opening = reply.trim_start_matches(['*', '#', '`', ' ']);
    FINAL_OPENINGS.iter().any(|word| opening.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric())))
}

fn last_lines(lines: &[Entry]) -> String {
    let tail = &lines[lines.len().saturating_sub(LAST_LINES)..];
    let rendered: Vec<String> = tail.iter().map(|entry| format!("{}: {}", entry.kind.label(), summarize(&entry.text.replace('\n', " "), LINE_CHARS))).collect();
    format!("Last {} transcript lines:\n{}", tail.len(), rendered.join("\n"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use crate::fleet::model::EntryKind;

    use super::*;

    #[derive(Default)]
    struct Book(Mutex<HashSet<(String, String)>>);

    impl ReceiptBook for Book {
        fn claim_receipt(&self, session: &str, key: &str, _claimed_ms: u64) -> NorthResult<bool> {
            Ok(self.0.lock().unwrap().insert((session.to_owned(), key.to_owned())))
        }
    }

    #[derive(Default)]
    struct Inbox(Mutex<Vec<(String, String, Delivery)>>);

    impl Courier for Inbox {
        fn post(&self, parent: &str, text: String, delivery: Delivery) {
            self.0.lock().unwrap().push((parent.to_owned(), text, delivery));
        }

        fn ask(&self, _request: GateRequest) {}

        fn answer(&self, _host_dir: &Path, _frame: String) {}
    }

    fn report(exit: TurnExit, reply: &str) -> Report {
        Report { child: "kid".into(), turn: 3, exit, landed: Some("abc123".into()), reply: Some(reply.into()) }
    }

    fn lines(count: usize) -> Vec<Entry> {
        (1..=count).map(|n| Entry { kind: EntryKind::Tool, text: format!("step {n}\nmore") }).collect()
    }

    fn child(stopped: bool, host_pid: Option<u32>) -> SessionRecord {
        serde_json::from_value(serde_json::json!({
            "name": "kid", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0,
            "host_pid": host_pid, "conversation": "s", "parent": "boss", "stopped": stopped
        }))
        .unwrap()
    }

    #[test]
    fn a_completed_turn_reports_its_reply_to_the_parent_once_and_a_long_one_points_at_show() {
        let (book, inbox) = (Book::default(), Inbox::default());
        assert!(deliver_report(&book, &inbox, "boss", &report(TurnExit::Completed, " done: 4 files \n"), &[], 1).unwrap());
        assert!(!deliver_report(&book, &inbox, "boss", &report(TurnExit::Completed, "done again"), &[], 2).unwrap());
        assert!(!deliver_report(&book, &inbox, "boss", &report(TurnExit::Interrupted, "partial"), &[], 3).unwrap());
        let long = Report { turn: 4, landed: None, ..report(TurnExit::Completed, &format!("Done: {}", "x".repeat(REPORT_CHARS))) };
        deliver_report(&book, &inbox, "boss", &long, &[], 4).unwrap();
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted[0], ("boss".to_owned(), "[from kid · turn 3 · exit completed · landed abc123] done: 4 files".to_owned(), Delivery::Queued));
        assert_eq!(posted.len(), 2);
        assert!(posted[1].1.starts_with("[from kid · turn 4 · exit completed] Done: xxx"), "{}", posted[1].1);
        assert!(posted[1].1.ends_with("…\n[report truncated; read it all with `north fleet show kid`]"), "{}", posted[1].1);
    }

    #[test]
    fn a_progress_turn_stays_out_of_the_parent_while_final_failed_and_landed_turns_are_posted() {
        let progress = Report { landed: None, ..report(TurnExit::Completed, "Still waiting on the run") };
        let turns = [
            (progress.clone(), false),
            (Report { reply: Some("Which target?".into()), ..progress.clone() }, false),
            (Report { reply: Some("Landed abc1234".into()), ..progress.clone() }, false),
            (Report { reply: Some("Donegal is a county".into()), ..progress.clone() }, false),
            (Report { reply: Some("Stopping to think".into()), ..progress.clone() }, false),
            (Report { landed: Some("abc1234".into()), ..progress.clone() }, true),
            (Report { exit: TurnExit::Failed, ..progress.clone() }, true),
            (Report { reply: Some("Done: 4 files".into()), ..progress.clone() }, true),
            (Report { reply: Some("Not done: the run failed\nStop: landing".into()), ..progress.clone() }, true),
            (Report { reply: Some("Split: two parts".into()), ..progress.clone() }, true),
            (Report { reply: Some("Stop: decision".into()), ..progress.clone() }, true),
            (Report { reply: Some("Blocked: needs a sign-in".into()), ..progress.clone() }, true),
            (Report { reply: Some("**Done:** 4 files".into()), ..progress }, true),
        ];
        for (n, (report, posted)) in turns.into_iter().enumerate() {
            let (book, inbox) = (Book::default(), Inbox::default());
            assert_eq!(deliver_report(&book, &inbox, "boss", &report, &lines(2), n as u64).unwrap(), posted, "{report:?}");
            assert_eq!(inbox.0.into_inner().unwrap().len(), usize::from(posted), "{report:?}");
        }
    }

    #[test]
    fn a_failed_turn_is_urgent_and_carries_the_last_five_transcript_lines() {
        let (book, inbox) = (Book::default(), Inbox::default());
        let failed = report(TurnExit::Failed, "ignored");
        assert!(!deliver_report(&book, &inbox, "boss", &Report { reply: None, ..failed.clone() }, &lines(7), 0).unwrap());
        deliver_report(&book, &inbox, "boss", &failed, &lines(7), 1).unwrap();
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted.len(), 1);
        let (_, text, delivery) = &posted[0];
        assert_eq!(*delivery, Delivery::Now);
        assert!(text.starts_with("[from kid · turn 3 · exit failed · landed abc123] The turn failed. Last 5 transcript lines:\ntool: step 3 more\n"), "{text}");
        assert!(text.ends_with("tool: step 7 more"), "{text}");
    }

    #[test]
    fn a_watchdog_lets_go_once_its_child_is_stopped_forgotten_or_on_a_new_host() {
        assert!(still_watched(Some(&child(false, Some(41))), 41));
        assert!(!still_watched(Some(&child(true, Some(41))), 41));
        assert!(!still_watched(Some(&child(false, Some(42))), 41));
        assert!(!still_watched(None, 41), "a forgotten child on a shared host that lives on needs no watchdog");
    }

    #[test]
    fn a_host_that_dies_mid_task_alarms_the_parent_once_unless_it_was_stopped_or_restarted() {
        let (book, inbox) = (Book::default(), Inbox::default());
        let running = child(false, Some(41));
        let death = |child: &SessionRecord, mid_task| deliver_death(&book, &inbox, &HostDeath { child, host_pid: 41, mid_task, lines: &lines(2) }, 9).unwrap();
        assert!(!death(&child(true, Some(41)), true), "a stopped child was ended on purpose");
        assert!(!death(&child(false, Some(42)), true), "a restarted child has a new host and its own watchdog");
        assert!(!death(&running, false), "a child that died idle had already reported");
        assert!(death(&running, true));
        assert!(!death(&running, true), "a second watchdog of the same host stays quiet");
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted, [("boss".to_owned(), "[from kid · host 41 exited mid-task] Last 2 transcript lines:\ntool: step 1 more\ntool: step 2 more".to_owned(), Delivery::Now)]);
    }
}
