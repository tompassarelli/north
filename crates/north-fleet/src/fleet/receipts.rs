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
(D is 30s, 10m, 4h or 1d; it prints the gate id, and the default applies when nobody answers in time; a question about cost, accounts, credentials or access, making something public, deleting valuable data, or core direction and dependencies is refused with --default and --default-after: open it without them and it waits for Tom). \
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
    let Some(at) = gate.default_at_ms() else {
        return format!("[gate {} is open: recommends {} ({}); it has no default and waits for Tom's answer, given with `north gate answer {} N`]", gate.id, request.recommend, option(request.recommend), gate.id);
    };
    let left = at.saturating_sub(now_ms) / 60_000;
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
        fn notify(&self, parent: &str, text: String, delivery: Delivery) {
            self.0.lock().unwrap().push((parent.to_owned(), text, delivery));
        }

        fn ask(&self, _request: GateRequest) {}

        fn answer(&self, _host_dir: &Path, _frame: String) {}
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
