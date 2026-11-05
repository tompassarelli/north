use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::error::NorthResult;

use super::gates::{Gate, GateBook, GateRequest};
use super::model::{Delivery, Entry, SessionRecord};
use super::runtime::summarize;
use super::turns::TurnExit;

const REPORT_CHARS: usize = 4_000;
const LINE_CHARS: usize = 300;
const LAST_LINES: usize = 5;
const FINAL_OPENINGS: [&str; 5] = ["Done", "Not done", "Split", "Stop", "Blocked"];
const DECISION_REPLY_KEY: &str = "decision-reply";
const DECISION_REPLY: &str = "North did not pass your report to your parent: it stops on a decision but names no open gate. \
Routine calls are decided, not asked: pick the option you would recommend, do the work and report the result. \
If only Tom can decide, open a gate and report again naming its id (`gate <id>`): \
`north gate open --session <your session> --question <question> --option <option> --option <option> --recommend N --default N --default-after D` \
(D is 30s, 10m, 4h or 1d; it prints the gate id, and the default applies when nobody answers in time). \
If you report again unchanged, the report goes to your parent with a warning.";
const DECISION_WARNING: &str = "[warning: this report stops on a decision but names no open gate; the child was told to decide routine calls or open one, and reported again]";
static DECISION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:\A\W*Blocked:\s*decision\b|(?m:^\W*Stop:\s*decision\b))").expect("decision stop pattern"));
static GATE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bgate\s*#?\s*(\d+)\b").expect("gate id pattern"));
static NEXT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^[\W_]*Next:[*_` \t]*(.*?)[ \t]*$").expect("next line pattern"));
static ISSUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:(?:\b[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)?)?#\d+\b|github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/issues/\d+)").expect("issue pattern"));
static NONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\A[*_`]*none\b").expect("none pattern"));

/// What a report's last `Next:` line says owns the step after it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Next {
    Missing,
    Nothing,
    Issue(String),
    Unowned(String),
}

impl Next {
    pub fn of(reply: &str) -> Self {
        let Some(found) = NEXT_LINE.captures_iter(reply).last() else { return Self::Missing };
        let text = found[1].trim();
        if let Some(issue) = ISSUE.find(text) {
            return Self::Issue(issue.as_str().to_owned());
        }
        if NONE.is_match(text) { Self::Nothing } else { Self::Unowned(text.to_owned()) }
    }

    fn owned(&self) -> bool {
        matches!(self, Self::Nothing | Self::Issue(_))
    }
}

/// Whether a report lets its session retire: it opens `Done:` and its `Next:` is `none` or names the issue that owns the next step.
pub fn retires(reply: &str) -> bool {
    opens(reply, "Done") && Next::of(reply).owned()
}

pub trait ReceiptBook: Send + Sync {
    fn claim_receipt(&self, session: &str, key: &str, claimed_ms: u64) -> NorthResult<bool>;
}

pub trait Courier: Send + Sync {
    fn notify(&self, parent: &str, text: String, delivery: Delivery);
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
            TurnExit::Failed => Some((format!("{} The turn failed. {}", self.header(), last_lines(lines)), Delivery::Queued)),
            TurnExit::Interrupted | TurnExit::Killed => None,
        }
    }

    fn asks_a_decision(&self) -> bool {
        self.exit == TurnExit::Completed && DECISION.is_match(self.reply.as_deref().unwrap_or_default().trim_start())
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

pub fn deliver_report(book: &dyn ReceiptBook, gates: &dyn GateBook, courier: &dyn Courier, parent: &str, report: &Report, lines: &[Entry], now_ms: u64) -> NorthResult<bool> {
    let Some((text, delivery)) = report.notice(lines) else { return Ok(false) };
    let text = if report.asks_a_decision() {
        match open_gate_named(gates, report.reply.as_deref().unwrap_or_default())? {
            Some(gate) => format!("{text}\n{}", gate_line(&gate, now_ms)),
            None if book.claim_receipt(&report.child, DECISION_REPLY_KEY, now_ms)? => {
                courier.notify(&report.child, DECISION_REPLY.to_owned(), Delivery::Now);
                return Ok(false);
            }
            None => format!("{text}\n{DECISION_WARNING}"),
        }
    } else {
        text
    };
    let claimed = book.claim_receipt(&report.child, &report.key(), now_ms)?;
    if claimed {
        courier.notify(parent, text, delivery);
    }
    Ok(claimed)
}

fn open_gate_named(gates: &dyn GateBook, reply: &str) -> NorthResult<Option<Gate>> {
    for id in GATE_ID.captures_iter(reply).filter_map(|found| found[1].parse::<i64>().ok()) {
        if let Some(gate) = gates.gate(id)?.filter(Gate::is_open) {
            return Ok(Some(gate));
        }
    }
    Ok(None)
}

fn gate_line(gate: &Gate, now_ms: u64) -> String {
    let option = |number: usize| gate.option(number).unwrap_or_default();
    let request = &gate.request;
    let left = gate.default_at_ms().saturating_sub(now_ms) / 60_000;
    format!("[gate {} is open: recommends {} ({}); defaults to {} ({}) in {left}m unless answered with `north gate answer {} N`]", gate.id, request.recommend, option(request.recommend), request.default, option(request.default), gate.id)
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
        courier.notify(parent, text, Delivery::Now);
    }
    Ok(claimed)
}

pub(super) fn is_final(reply: &str) -> bool {
    FINAL_OPENINGS.iter().any(|word| opens(reply, word))
}

fn opens(reply: &str, word: &str) -> bool {
    let opening = reply.trim_start_matches(['*', '#', '`', ' ']);
    opening.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
}

pub(super) fn last_lines(lines: &[Entry]) -> String {
    let tail = &lines[lines.len().saturating_sub(LAST_LINES)..];
    let rendered: Vec<String> = tail.iter().map(|entry| format!("{}: {}", entry.kind.label(), summarize(&entry.text.replace('\n', " "), LINE_CHARS))).collect();
    format!("Last {} transcript lines:\n{}", tail.len(), rendered.join("\n"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use crate::fleet::gates::Closure;
    use crate::fleet::model::EntryKind;

    use super::*;

    #[derive(Default)]
    struct Book(Mutex<HashSet<(String, String)>>);

    impl ReceiptBook for Book {
        fn claim_receipt(&self, session: &str, key: &str, _claimed_ms: u64) -> NorthResult<bool> {
            Ok(self.0.lock().unwrap().insert((session.to_owned(), key.to_owned())))
        }
    }

    struct Gates(Vec<Gate>);

    impl GateBook for Gates {
        fn open_gate(&self, _request: &GateRequest, _opened_ms: u64) -> NorthResult<i64> {
            Ok(0)
        }

        fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
            Ok(self.0.iter().find(|gate| gate.id == id).cloned())
        }

        fn gates(&self, _closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
            Ok(self.0.clone())
        }

        fn close_gate(&self, _id: i64, _closure: &Closure) -> NorthResult<bool> {
            Ok(false)
        }
    }

    #[derive(Default)]
    struct Inbox(Mutex<Vec<(String, String, Delivery)>>);

    impl Courier for Inbox {
        fn notify(&self, parent: &str, text: String, delivery: Delivery) {
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
        assert!(deliver_report(&book, &Gates(vec![]), &inbox, "boss", &report(TurnExit::Completed, " done: 4 files \n"), &[], 1).unwrap());
        assert!(!deliver_report(&book, &Gates(vec![]), &inbox, "boss", &report(TurnExit::Completed, "done again"), &[], 2).unwrap());
        assert!(!deliver_report(&book, &Gates(vec![]), &inbox, "boss", &report(TurnExit::Interrupted, "partial"), &[], 3).unwrap());
        let long = Report { turn: 4, landed: None, ..report(TurnExit::Completed, &format!("Done: {}", "x".repeat(REPORT_CHARS))) };
        deliver_report(&book, &Gates(vec![]), &inbox, "boss", &long, &[], 4).unwrap();
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
            (Report { reply: Some("Blocked: needs a sign-in".into()), ..progress.clone() }, true),
            (Report { reply: Some("**Done:** 4 files".into()), ..progress }, true),
        ];
        for (n, (report, posted)) in turns.into_iter().enumerate() {
            let (book, inbox) = (Book::default(), Inbox::default());
            assert_eq!(deliver_report(&book, &Gates(vec![]), &inbox, "boss", &report, &lines(2), n as u64).unwrap(), posted, "{report:?}");
            assert_eq!(inbox.0.into_inner().unwrap().len(), usize::from(posted), "{report:?}");
        }
    }

    fn gate(id: i64, closure: Option<Closure>) -> Gate {
        let request = GateRequest { verification: false, issue: None, session: "kid".into(), question: "Rent the box?".into(), options: vec!["rent".into(), "wait".into()], recommend: 1, default: 2, default_after_ms: 1_800_000 };
        Gate { id, request, opened_ms: 0, closure }
    }

    fn stopped(reply: &str) -> Report {
        Report { landed: None, ..report(TurnExit::Completed, reply) }
    }

    #[test]
    fn a_decision_stop_with_no_open_gate_is_answered_by_the_fleet_once_and_not_forwarded() {
        let (book, inbox) = (Book::default(), Inbox::default());
        let closed = Closure { answer: "wait".into(), by: crate::fleet::gates::Answerer::Default, closed_ms: 1 };
        let gates = Gates(vec![gate(7, Some(closed))]);
        for (n, reply) in ["Not done: needs a call\nStop: decision", "Blocked: decision on the port", "Not done: asked in gate 7\nStop: decision", "Not done: x\nStop: decision (gate 99)"].into_iter().enumerate() {
            let book = Book::default();
            let inbox = Inbox::default();
            let report = Report { turn: n as u64, ..stopped(reply) };
            assert!(!deliver_report(&book, &gates, &inbox, "boss", &report, &[], 1).unwrap(), "{reply}");
            let posted = inbox.0.into_inner().unwrap();
            assert_eq!(posted.len(), 1, "{reply}");
            let (to, text, delivery) = &posted[0];
            assert_eq!((to.as_str(), *delivery), ("kid", Delivery::Now));
            assert!(text.contains("north gate open --session <your session> --question <question> --option <option> --option <option> --recommend N --default N --default-after D"), "{text}");
            assert!(text.contains("decided, not asked"), "{text}");
        }
        assert!(deliver_report(&book, &gates, &inbox, "boss", &stopped("Done: fine\nnot a stop"), &[], 1).unwrap());
        assert!(deliver_report(&book, &gates, &inbox, "boss", &Report { turn: 9, ..stopped("Not done: x. Blocked: decision is not the opening.") }, &[], 2).unwrap());
    }

    #[test]
    fn a_decision_stop_naming_an_open_gate_is_forwarded_with_its_recommendation_and_default_time() {
        let (book, inbox) = (Book::default(), Inbox::default());
        let gates = Gates(vec![gate(7, None)]);
        assert!(deliver_report(&book, &gates, &inbox, "boss", &stopped("Not done: the box needs Tom.\nStop: decision (gate 7)"), &[], 60_000).unwrap());
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].0, "boss");
        assert!(posted[0].1.starts_with("[from kid · turn 3 · exit completed] Not done: the box needs Tom.\nStop: decision (gate 7)\n"), "{}", posted[0].1);
        assert!(posted[0].1.ends_with("[gate 7 is open: recommends 1 (rent); defaults to 2 (wait) in 29m unless answered with `north gate answer 7 N`]"), "{}", posted[0].1);
    }

    #[test]
    fn a_second_decision_stop_without_a_gate_is_forwarded_with_a_warning_so_nothing_deadlocks() {
        let (book, inbox) = (Book::default(), Inbox::default());
        let gates = Gates(vec![]);
        let first = stopped("Not done: x\nStop: decision");
        assert!(!deliver_report(&book, &gates, &inbox, "boss", &first, &[], 1).unwrap());
        assert!(deliver_report(&book, &gates, &inbox, "boss", &Report { turn: 4, ..first }, &[], 2).unwrap());
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted.len(), 2);
        assert_eq!(posted[0].0, "kid");
        assert_eq!(posted[1].0, "boss");
        assert!(posted[1].1.contains("Stop: decision") && posted[1].1.ends_with(DECISION_WARNING), "{}", posted[1].1);
    }

    #[test]
    fn a_failed_turn_is_queued_like_any_receipt_and_carries_the_last_five_transcript_lines() {
        let (book, inbox) = (Book::default(), Inbox::default());
        let failed = report(TurnExit::Failed, "ignored");
        assert!(!deliver_report(&book, &Gates(vec![]), &inbox, "boss", &Report { reply: None, ..failed.clone() }, &lines(7), 0).unwrap());
        deliver_report(&book, &Gates(vec![]), &inbox, "boss", &failed, &lines(7), 1).unwrap();
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted.len(), 1);
        let (_, text, delivery) = &posted[0];
        assert_eq!(*delivery, Delivery::Queued);
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

    #[test]
    fn the_last_next_line_names_none_an_issue_or_a_step_nobody_owns() {
        let cases = [
            ("Done: x\nNext: none", Next::Nothing),
            ("Done: x\n- **Next:** None.", Next::Nothing),
            ("Done: x\nNext: north#600 owns the bench", Next::Issue("north#600".into())),
            ("Done: x\nNext: tompassarelli/north#12", Next::Issue("tompassarelli/north#12".into())),
            ("Done: x\nNext: #41", Next::Issue("#41".into())),
            ("Done: x\nNext: see https://github.com/tompassarelli/north/issues/7", Next::Issue("github.com/tompassarelli/north/issues/7".into())),
            ("Done: x\nNext: run the bench", Next::Unowned("run the bench".into())),
            ("Done: x\nNext: north#1\nNext: rerun it", Next::Unowned("rerun it".into())),
            ("Done: x\nNext:", Next::Unowned(String::new())),
            ("Done: x", Next::Missing),
            ("Done: x. Next: none", Next::Missing),
        ];
        for (reply, next) in cases {
            assert_eq!(Next::of(reply), next, "{reply}");
        }
        assert!(retires("**Done:** x\nNext: none"));
        assert!(!retires("Donegal\nNext: none"));
        assert!(!retires("Split: x\nNext: none"));
    }
}
