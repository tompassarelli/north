use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::error::NorthResult;

use super::ask_once;
use super::gates::{Gate, GateBook, GateRequest};
use super::model::{Delivery, Entry, SessionRecord};
use super::runtime::summarize;
use super::turns::TurnExit;
pub use super::upward::is_final;
use super::upward::{Facts, opens};

const REPORT_CHARS: usize = 4_000;
const LINE_CHARS: usize = 300;
const LAST_LINES: usize = 5;
const MID_TASK_REVIVE_WINDOW_MS: u64 = 60 * 60 * 1000;
const DECISION_REPLY_KEY: &str = "decision-reply";
const DECISION_REPLY: &str = "North did not pass your report to your parent: it stops on a decision, and only an ask-once matter (cost, accounts, credentials or access, making something public, deleting valuable data, sending in Tom's name, outside code of unclear license, or a core-project direction call) waits for Tom. \
Act on your recommendation, record it as a gate with a default, and continue the work: \
`north gate open --session <your session> --question <question> --option <option> --option <option> --recommend N --default N --default-after D` \
(D is 30s, 10m, 4h or 1d; it prints the gate id, and the default applies when nobody answers in time; a question about cost, accounts, credentials or access, making something public, deleting valuable data, or core direction and dependencies is refused with --default and --default-after: open it without them and it waits for Tom). \
A report that stops on an ask-once gate names its id (`gate <id>`). If you report again unchanged, the report goes to your parent with a warning.";
const DECISION_WARNING: &str = "[warning: this report stops on a decision that is not an ask-once matter; the child was told to act on its recommendation, and reported again]";
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
    fn reported(&self, session: &str) -> NorthResult<bool>;
    fn last_receipt(&self, session: &str, prefix: &str) -> NorthResult<Option<String>>;
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
    pub waiting: bool,
}

impl Report {
    fn notice(&self, facts: &Facts, lines: &[Entry]) -> Option<(String, Delivery)> {
        let reply = self.reply.as_deref().unwrap_or_default().trim();
        if reply.is_empty() {
            return None;
        }
        match self.exit {
            TurnExit::Completed if facts.worth_telling() => Some((self.receipt(), Delivery::Queued)),
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
    let open: BTreeSet<i64> = gates.gates(u64::MAX)?.into_iter().filter(|gate| gate.is_open() && gate.request.session == report.child).map(|gate| gate.id).collect();
    let facts = Facts::of(report.reply.as_deref().unwrap_or_default().trim(), report.landed.as_deref(), open, report.waiting);
    let Some((text, delivery)) = report.notice(&facts, lines) else { return Ok(false) };
    let told = (report.exit == TurnExit::Completed).then_some(facts);
    if let Some(facts) = &told
        && facts.repeats(book.last_receipt(&report.child, Facts::PREFIX)?.as_deref())
    {
        return Ok(false);
    }
    let text = if report.asks_a_decision() {
        match ask_once_gate_named(gates, report.reply.as_deref().unwrap_or_default())? {
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
        if let Some(facts) = told {
            book.claim_receipt(&report.child, &facts.receipt(report.turn), now_ms)?;
        }
    }
    Ok(claimed)
}

fn ask_once_gate_named(gates: &dyn GateBook, reply: &str) -> NorthResult<Option<Gate>> {
    for id in GATE_ID.captures_iter(reply).filter_map(|found| found[1].parse::<i64>().ok()) {
        if let Some(gate) = gates.gate(id)?.filter(|gate| gate.is_open() && ask_once::class(&gate.request.question, &gate.request.options).is_some()) {
            return Ok(Some(gate));
        }
    }
    Ok(None)
}

fn gate_line(gate: &Gate, now_ms: u64) -> String {
    let option = |number: usize| gate.option(number).unwrap_or_default();
    let request = &gate.request;
    let Some(at) = gate.default_at_ms() else {
        return format!("[gate {} is open: recommends {} ({}); it has no default and waits for Tom, who answers by telling the proxy]", gate.id, request.recommend, option(request.recommend));
    };
    let left = at.saturating_sub(now_ms) / 60_000;
    format!("[gate {} is open: recommends {} ({}); defaults to {} ({}) in {left}m unless Tom answers by telling the proxy]", gate.id, request.recommend, option(request.recommend), request.default, option(request.default))
}

pub struct HostDeath<'a> {
    pub child: &'a SessionRecord,
    pub host_pid: u32,
    pub mid_task: bool,
    pub lines: &'a [Entry],
}

pub fn still_watched(child: Option<&SessionRecord>, host_pid: u32) -> bool {
    child.is_some_and(|child| !child.ended() && child.host_pid == Some(host_pid))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Aftermath {
    Quiet,
    Revive,
    Reported,
}

pub fn deliver_death(book: &dyn ReceiptBook, courier: &dyn Courier, death: &HostDeath, now_ms: u64) -> NorthResult<Aftermath> {
    let child = death.child;
    let restarted = child.host_pid != Some(death.host_pid);
    if child.ended() || restarted || !death.mid_task {
        return Ok(Aftermath::Quiet);
    }
    if child.revived_mid_task_ms.is_none_or(|at| now_ms.saturating_sub(at) >= MID_TASK_REVIVE_WINDOW_MS) {
        return Ok(Aftermath::Revive);
    }
    let Some(parent) = child.parent.as_deref() else { return Ok(Aftermath::Quiet) };
    if !book.claim_receipt(&child.name, &format!("host:{}", death.host_pid), now_ms)? {
        return Ok(Aftermath::Quiet);
    }
    let text = format!("[from {} · host {} exited mid-task again within the hour of its revive] {}", child.name, death.host_pid, last_lines(death.lines));
    courier.notify(parent, text, Delivery::Now);
    Ok(Aftermath::Reported)
}

pub(super) fn last_lines(lines: &[Entry]) -> String {
    let tail = &lines[lines.len().saturating_sub(LAST_LINES)..];
    let rendered: Vec<String> = tail.iter().map(|entry| format!("{}: {}", entry.kind.label(), summarize(&entry.text.replace('\n', " "), LINE_CHARS))).collect();
    format!("Last {} transcript lines:\n{}", tail.len(), rendered.join("\n"))
}

#[cfg(test)]
mod tests {
        use std::sync::Mutex;

    use crate::fleet::model::{EntryKind, Phase};

    use super::*;

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

    fn child(stopped: bool, host_pid: Option<u32>, revived_mid_task_ms: Option<u64>) -> SessionRecord {
        let mut child: SessionRecord = serde_json::from_value(serde_json::json!({
            "name": "kid", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0,
            "host_pid": host_pid, "conversation": "s", "parent": "boss", "revived_mid_task_ms": revived_mid_task_ms
        }))
        .unwrap();
        child.phase = Some(if stopped { Phase::Stopped } else { Phase::Live });
        child
    }

    #[test]
    fn a_host_death_mid_task_revives_once_an_hour_then_tells_the_parent_once_and_a_decision_stop_reaches_it_only_through_an_ask_once_gate() {
        let (book, inbox) = (crate::fleet::fakes::Paper::default(), Inbox::default());
        let hour = MID_TASK_REVIVE_WINDOW_MS;
        let death = |child: &SessionRecord, mid_task, now| deliver_death(&book, &inbox, &HostDeath { child, host_pid: 41, mid_task, lines: &lines(2) }, now).unwrap();
        assert_eq!(death(&child(true, Some(41), None), true, 9), Aftermath::Quiet, "a stopped child was ended on purpose");
        assert_eq!(death(&child(false, Some(42), None), true, 9), Aftermath::Quiet, "a restarted child has a new host and its own watchdog");
        assert_eq!(death(&child(false, Some(41), None), false, 9), Aftermath::Quiet, "a child that died idle had already reported");
        assert_eq!(death(&child(false, Some(41), None), true, 9), Aftermath::Revive, "north-743..746-d0351 waited for a hand revive");
        assert_eq!(death(&child(false, Some(41), Some(1)), true, hour + 1), Aftermath::Revive, "a revive over an hour ago does not count");
        let crashing = child(false, Some(41), Some(1));
        assert_eq!(death(&crashing, true, hour), Aftermath::Reported);
        assert_eq!(death(&crashing, true, hour), Aftermath::Quiet, "a second watchdog of the same host stays quiet");
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted, [("boss".to_owned(), "[from kid · host 41 exited mid-task again within the hour of its revive] Last 2 transcript lines:\ntool: step 1 more\ntool: step 2 more".to_owned(), Delivery::Now)]);
        let (book, inbox) = (crate::fleet::fakes::Paper::default(), Inbox::default());
        let gate = |question: &str, default: usize, default_after_ms: u64| GateRequest { verification: false, issue: None, box_text: None, session: "kid".into(), question: question.into(), options: vec!["yes".into(), "no".into()], recommend: 1, default, default_after_ms };
        let routine = book.open_gate(&gate("Which module name reads better?", 1, 60_000), 0).unwrap();
        let spending = book.open_gate(&gate("May we turn on extra usage credits?", 0, 0), 0).unwrap();
        let report = |turn, reply: &str| Report { child: "kid".into(), turn, exit: TurnExit::Completed, landed: None, reply: Some(reply.into()), waiting: false };
        let deliver = |report: &Report| deliver_report(&book, &book, &inbox, "boss", report, &[], 1).unwrap();

        assert!(!deliver(&report(1, &format!("Blocked: decision\nStop: decision, gate {routine}"))), "a gate with a default is no ask-once matter");
        assert!(deliver(&report(2, &format!("Blocked: decision\nStop: decision, gate {spending}"))));
        let posted = inbox.0.into_inner().unwrap();
        assert_eq!(posted.len(), 2);
        assert_eq!((posted[0].0.as_str(), posted[0].2), ("kid", Delivery::Now));
        assert!(posted[0].1.contains("Act on your recommendation, record it as a gate with a default, and continue"), "{}", posted[0].1);
        assert_eq!(posted[1].0, "boss");
        assert!(posted[1].1.contains(&format!("[gate {spending} is open")) && !posted[1].1.contains("[warning"), "{}", posted[1].1);
    }

    #[test]
    fn a_report_reaches_the_parent_only_with_a_new_fact_so_a_wait_on_its_own_queued_ticket_or_a_repeat_is_nothing() {
        let (book, inbox) = (crate::fleet::fakes::Paper::default(), Inbox::default());
        let report = |turn, landed: Option<&str>, waiting, reply: &str| Report { child: "land-lead".into(), turn, exit: TurnExit::Completed, landed: landed.map(str::to_owned), reply: Some(reply.into()), waiting };
        let deliver = |report: Report| deliver_report(&book, &book, &inbox, "proxy", &report, &[], 1).unwrap();

        assert!(!deliver(report(39, None, true, "Not done: the lander still runs dab9f6fcf4, behind current main.")), "north#909: land-lead sent this turn after turn on 2026-11-08 while its ticket sat queued");
        assert!(deliver(report(40, None, false, "Not done: the build host refused the lease.")));
        assert!(!deliver(report(41, None, false, "Not done: the build host refused the lease again.")), "a repeat of the last report that went up");
        assert!(deliver(report(42, Some("be5886ab3b"), true, "Not done: half of it landed.")), "a landing always goes up");
        assert!(deliver(report(43, None, false, "Not done: the build host refused the lease.")), "the same stop after a different report is news again");
        assert!(deliver(report(44, None, true, "Done: ticked the box with be5886ab3b.\nNext: none")));
        assert_eq!(inbox.0.into_inner().unwrap().len(), 4);
    }
}
