use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::error::NorthResult;

use super::ask_once;
use super::gates::{Gate, GateBook, GateRequest};
use super::model::{Delivery, Entry, SessionRecord};
use super::runtime::{PROXY, summarize};
use super::turns::TurnExit;
pub use super::upward::is_final;
use super::upward::{Facts, opens};

const REPORT_CHARS: usize = 4_000;
const HEAD_LINES: usize = 3;
const LINE_CHARS: usize = 300;
const LAST_LINES: usize = 5;
const MID_TASK_REVIVE_WINDOW_MS: u64 = 60 * 60 * 1000;
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
    /// Keeps a report that opens nothing for `parent` in the digest instead of waking it.
    fn hold(&self, _parent: &str, _text: String) {}
    /// Starts `north fleet launch WORDS`, as a two-rung route's second attempt.
    fn launch(&self, _words: Vec<String>) {}
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
    fn notice(&self, lines: &[Entry], head_only: bool) -> Option<(String, Delivery)> {
        let reply = self.reply.as_deref().unwrap_or_default().trim();
        if reply.is_empty() {
            return None;
        }
        match self.exit {
            TurnExit::Completed => Some((self.receipt(head_only), Delivery::Queued)),
            TurnExit::Failed => Some((format!("{} The turn failed. {}", self.header(), last_lines(lines)), Delivery::Queued)),
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

    fn receipt(&self, head_only: bool) -> String {
        let reply = self.reply.as_deref().map(str::trim).unwrap_or_default();
        if head_only {
            return format!("{} {}\n[read it all with `north fleet show {}`]", self.header(), head(reply).join("\n"), self.child);
        }
        match reply.char_indices().nth(REPORT_CHARS) {
            Some((cut, _)) => format!("{} {}…\n[report truncated; read it all with `north fleet show {}`]", self.header(), &reply[..cut], self.child),
            None => format!("{} {reply}", self.header()),
        }
    }
}

fn head(reply: &str) -> Vec<String> {
    let lines: Vec<&str> = reply.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let mut head: Vec<&str> = lines.iter().take(HEAD_LINES).copied().collect();
    if let Some(stop) = lines.iter().find(|line| opens(line, "Stop"))
        && !head.contains(stop)
    {
        head.pop();
        head.push(stop);
    }
    head.into_iter().map(|line| summarize(line, LINE_CHARS)).collect()
}

pub fn deliver_report(book: &dyn ReceiptBook, gates: &dyn GateBook, courier: &dyn Courier, parent: &str, report: &Report, lines: &[Entry], now_ms: u64) -> NorthResult<bool> {
    let open: Vec<Gate> = gates.gates(u64::MAX)?.into_iter().filter(|gate| gate.is_open() && gate.request.session == report.child).collect();
    let facts = Facts::of(report.reply.as_deref().unwrap_or_default().trim(), report.landed.as_deref(), open.iter().map(|gate| gate.id).collect(), report.waiting);
    let Some((notice, delivery)) = report.notice(lines, parent == PROXY) else { return Ok(false) };
    let told = (report.exit == TurnExit::Completed).then_some(facts);
    let pages = match (&told, parent == PROXY) {
        (Some(facts), true) => facts.wakes_proxy(),
        (Some(facts), false) => facts.worth_telling(),
        (None, true) => false,
        (None, false) => true,
    };
    let repeats = match &told {
        Some(facts) => facts.repeats(book.last_receipt(&report.child, Facts::PREFIX)?.as_deref()),
        None => false,
    };
    if !pages || repeats {
        if book.claim_receipt(&report.child, &format!("held:{}", report.turn), now_ms)? {
            courier.hold(parent, notice);
        }
        return Ok(false);
    }
    let asked = open.iter().find(|gate| report.exit == TurnExit::Completed && ask_once::class(&gate.request.question, &gate.request.options).is_some());
    let text = match asked {
        Some(gate) => format!("{notice}\n{}", gate_line(gate, now_ms)),
        None => notice,
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
    struct Inbox {
        sent: Mutex<Vec<(String, String, Delivery)>>,
        held: Mutex<Vec<(String, String)>>,
    }

    impl Courier for Inbox {
        fn notify(&self, parent: &str, text: String, delivery: Delivery) {
            self.sent.lock().unwrap().push((parent.to_owned(), text, delivery));
        }

        fn ask(&self, _request: GateRequest) {}

        fn answer(&self, _host_dir: &Path, _frame: String) {}

        fn hold(&self, parent: &str, text: String) {
            self.held.lock().unwrap().push((parent.to_owned(), text));
        }
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
    fn a_host_death_mid_task_revives_once_an_hour_then_tells_the_parent_once_and_the_ask_once_gate_a_child_opened_reaches_it_beside_its_report() {
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
        let posted = inbox.sent.into_inner().unwrap();
        assert_eq!(posted, [("boss".to_owned(), "[from kid · host 41 exited mid-task again within the hour of its revive] Last 2 transcript lines:\ntool: step 1 more\ntool: step 2 more".to_owned(), Delivery::Now)]);
        let (book, inbox) = (crate::fleet::fakes::Paper::default(), Inbox::default());
        let gate = |session: &str, question: &str, default: usize, default_after_ms: u64| GateRequest { verification: false, issue: None, box_text: None, session: session.into(), question: question.into(), options: vec!["yes".into(), "no".into()], recommend: 1, default, default_after_ms, ask_once: None };
        book.open_gate(&gate("pal", "Which module name reads better?", 1, 60_000), 0).unwrap();
        let spending = book.open_gate(&gate("kid", "May we turn on extra usage credits?", 0, 0), 0).unwrap();
        let report = |child: &str, turn, reply: &str| Report { child: child.into(), turn, exit: TurnExit::Completed, landed: None, reply: Some(reply.into()), waiting: false };
        let deliver = |report: &Report| deliver_report(&book, &book, &inbox, "boss", report, &[], 1).unwrap();

        assert!(deliver(&report("pal", 1, "Blocked: which module name reads better.\nStop: decision")));
        assert!(deliver(&report("kid", 2, "Blocked: the run needs extra usage.")));
        let posted = inbox.sent.into_inner().unwrap();
        assert_eq!(posted.len(), 2);
        assert_eq!(posted[0].0, "boss");
        assert!(!posted[0].1.contains("[gate"), "a gate with a default is no ask-once matter, and a Stop: decision line opens none: {}", posted[0].1);
        assert_eq!(posted[1].0, "boss");
        assert!(posted[1].1.contains(&format!("[gate {spending} is open")), "the ask-once gate the child opened reaches its parent, whatever its report names: {}", posted[1].1);
    }

    #[test]
    fn a_report_to_the_proxy_is_its_head_only_and_reaches_it_only_with_a_new_fact_so_a_wait_on_its_own_queued_ticket_or_a_repeat_is_nothing() {
        let (book, inbox) = (crate::fleet::fakes::Paper::default(), Inbox::default());
        let report = |turn, landed: Option<&str>, waiting, reply: &str| Report { child: "land-lead".into(), turn, exit: TurnExit::Completed, landed: landed.map(str::to_owned), reply: Some(reply.into()), waiting };
        let deliver = |report: Report| deliver_report(&book, &book, &inbox, "proxy", &report, &[], 1).unwrap();

        assert!(!deliver(report(39, None, true, "Not done: the lander still runs dab9f6fcf4, behind current main.")), "north#909: land-lead sent this turn after turn on 2026-11-08 while its ticket sat queued");
        assert!(deliver(report(40, None, false, "Not done: the build host refused the lease.")));
        assert!(!deliver(report(41, None, false, "Not done: the build host refused the lease again.")), "a repeat of the last report that went up");
        assert!(deliver(report(42, Some("be5886ab3b"), true, "Not done: half of it landed.")), "a landing always goes up");
        assert!(deliver(report(43, None, false, "Not done: the build host refused the lease.")), "the same stop after a different report is news again");
        let done = "Done: ticked the box with be5886ab3b.\nBox: A health alarm pages once per cause.\nBox: Land-watch holds a landing behind its follower.\nEvidence: cargo nextest passed; clippy clean; origin/main carries be5886ab3b.\nNext: none";
        assert!(!deliver(report(44, Some("be5886ab3b"), true, done)), "a Done waits in the digest, not the proxy");
        let blocked = "Blocked: the broker token expired.\nEvidence: the last call answered 401 at 04:12.\nMore evidence: the token log shows no renewal.\nStop: wait until 2026-11-10T12:00Z";
        assert!(deliver(report(45, None, false, blocked)));
        let posted = inbox.sent.into_inner().unwrap();
        assert_eq!(posted.len(), 4);
        let decision = &posted[3].1;
        assert!(decision.contains("Blocked: the broker token expired."), "{decision}");
        assert!(decision.contains("Stop: wait until 2026-11-10T12:00Z"), "the head keeps the one decision: {decision}");
        assert!(!decision.contains("More evidence:"), "{decision}");
        assert_eq!(decision.lines().filter(|line| !line.starts_with("[read it all")).count(), 3, "{decision}");
        let held = inbox.held.into_inner().unwrap();
        let notice = &held.iter().find(|(_, text)| text.contains("Done: ticked")).unwrap().1;
        assert!(notice.contains("Done: ticked the box with be5886ab3b.") && notice.contains("landed be5886ab3b"), "{notice}");
        assert!(notice.contains("Box: A health alarm pages once per cause.") && notice.contains("Box: Land-watch holds a landing behind its follower."), "{notice}");
        assert!(!notice.contains("Evidence:") && !notice.contains("Next: none"), "the head leaves the evidence in the transcript: {notice}");
        assert!(notice.contains("[read it all with `north fleet show land-lead`]"), "{notice}");
        assert_eq!(notice.lines().filter(|line| !line.starts_with("[read it all")).count(), 3, "at most three head lines: {notice}");
    }

    #[test]
    fn a_workers_final_message_reaches_a_parent_mid_turn_exactly_once_within_60_s() {
        let (book, inbox) = (crate::fleet::fakes::Paper::default(), Inbox::default());
        let reported_ms = 1_000_000;
        let report = |turn, landed: Option<&str>, reply: &str| Report { child: "kid".into(), turn, exit: TurnExit::Completed, landed: landed.map(str::to_owned), reply: Some(reply.into()), waiting: false };
        let deliver = |report: &Report, now_ms| deliver_report(&book, &book, &inbox, "boss", report, &[], now_ms).unwrap();

        let finals = [
            report(1, None, "Done: the first box.\nNext: none"),
            report(2, Some("be5886ab3b"), "Done: the second box landed.\nNext: none"),
            report(3, None, "Done: the third box, nothing to land.\nNext: none"),
            report(4, None, "Done: the fourth box, nothing to land.\nNext: none"),
            report(5, None, "Split: the rest is north#1032.\nNext: north#1032"),
        ];
        for (turn, last) in finals.iter().enumerate() {
            let at = reported_ms + turn as u64 * 1_000;
            assert!(deliver(last, at), "turn {} of a worker whose parent is mid-turn is claimed on its own turn", last.turn);
            assert!(!deliver(last, at + 1), "a second delivery of turn {} claims nothing", last.turn);
        }
        let posted = inbox.sent.into_inner().unwrap();
        assert_eq!(posted.len(), finals.len(), "each final message reached the parent exactly once: {posted:?}");
        assert!(posted.iter().all(|(parent, _, delivery)| parent == "boss" && *delivery == Delivery::Queued));
        assert!(inbox.held.into_inner().unwrap().is_empty(), "none was kept back for the digest");
    }

    #[test]
    fn only_the_page_now_classes_wake_the_proxy() {
        use crate::fleet::fold::{Admit, admit};
        use crate::fleet::sender::Sender;

        let home = tempfile::tempdir().unwrap();
        let service = |unit: &str| Sender::Service(unit.to_owned());
        let alarm = |sender: &Sender, text: &str| admit(home.path(), sender, "proxy", text, None, 0);
        assert_eq!(alarm(&Sender::Tom, "gate 7 is open: recommends 1"), Admit::Send, "Tom's gate passes");
        assert_eq!(alarm(&service("north-ops-flow"), "north ops watch: north: possible red main: Cargo"), Admit::Send);
        assert_eq!(alarm(&service("north-runtime"), "north runtime canary: the launch probe failed; nexus rolled back to 11111111"), Admit::Send);
        assert_eq!(alarm(&service("north-capacity-watchdog"), "watchdog: host disk is past its high watermark, 3.1 GiB free"), Admit::Send);
        assert_eq!(alarm(&service("north-ops-flow"), "security finding: the credentials scan read a token in a brief"), Admit::Send);
        assert_eq!(alarm(&service("north-health"), "harness unit fleet-tick.service failed to start 3 times in the last hour: timed out"), Admit::Send);
        assert_eq!(alarm(&service("north-health"), "harness unit fleet-tick.service recovered: under 3 failed starts in the last hour"), Admit::Send);
        assert_eq!(alarm(&service("north-health"), "health: ci: recovered"), Admit::Hold, "a recovered check waits for the digest");
        assert_eq!(alarm(&service("north-lease-reaper"), "lease reaper: evicted slot gex45-3"), Admit::Hold, "an eviction waits for the digest");
        assert_eq!(alarm(&service("north-capacity-watchdog"), "watchdog: host overload, load 99.1 (4.1/core)"), Admit::Hold, "an overload waits for the digest");

        let (book, inbox) = (crate::fleet::fakes::Paper::default(), Inbox::default());
        let report = |turn, reply: &str| Report { child: "kid".into(), turn, exit: TurnExit::Completed, landed: None, reply: Some(reply.into()), waiting: false };
        let deliver = |report: Report| deliver_report(&book, &book, &inbox, "proxy", &report, &[], 1).unwrap();
        assert!(!deliver(report(1, "Done: all four boxes.")));
        assert!(!deliver(report(2, "Split: the rest is north#1032.")));
        assert!(deliver(report(3, "Blocked: the build host refused the lease.")));
        assert!(deliver(report(4, "Not done: the build host refused the lease.")));
        let failed = Report { child: "kid".into(), turn: 5, exit: TurnExit::Failed, landed: None, reply: Some("the provider exited".into()), waiting: false };
        assert!(!deliver(failed), "a failed turn waits for the digest too");
    }
}
