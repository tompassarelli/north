use std::cell::RefCell;

use super::check::{self, Alarms, Courier};
use super::expect::{Op, Outcome, State};
use super::facts::{Size, parse};

#[derive(Default)]
struct Recorder {
    phone: RefCell<Vec<String>>,
    proxy: RefCell<Vec<String>>,
    down: bool,
}

impl Courier for Recorder {
    fn phone(&self, title: &str, text: &str) -> Result<String, String> {
        if self.down {
            return Err("nexus unreachable".into());
        }
        self.phone.borrow_mut().push(format!("{title} | {text}"));
        Ok(format!("ntfy-{}", self.phone.borrow().len()))
    }

    fn proxy(&self, text: &str) -> Result<(), String> {
        self.proxy.borrow_mut().push(text.to_owned());
        Ok(())
    }
}

fn outcome(id: &str, state: State) -> Outcome {
    Outcome {
        id: id.into(),
        behavior: format!("{id} behaves"),
        metric: "m".into(),
        value: Some(1.0),
        op: Op::AtMost,
        threshold: 0.0,
        window: "now".into(),
        state,
        owner: "box".into(),
        fix: "fix".into(),
    }
}

#[test]
fn a_breach_pushes_once_and_tells_the_proxy_once_until_it_clears() {
    let courier = Recorder::default();
    let mut alarms = Alarms::new();
    let first = check::sound(&[outcome("landing-lock", State::Breached)], &mut alarms, &courier, 100);
    assert_eq!(first.raised, ["landing-lock"]);
    let again = check::sound(&[outcome("landing-lock", State::Breached)], &mut alarms, &courier, 400);
    assert!(again.raised.is_empty());
    assert_eq!(again.breached, ["landing-lock"]);
    assert_eq!(courier.phone.borrow().len(), 1);
    assert_eq!(courier.proxy.borrow().len(), 1);
    assert!(courier.proxy.borrow()[0].starts_with("[ops alert] landing-lock BREACHED"));
    let unknown = check::sound(&[outcome("landing-lock", State::Unknown)], &mut alarms, &courier, 700);
    assert!(unknown.cleared.is_empty() && alarms.contains_key("landing-lock"));
    let cleared = check::sound(&[outcome("landing-lock", State::Holds)], &mut alarms, &courier, 1000);
    assert_eq!(cleared.cleared, ["landing-lock"]);
    assert_eq!(courier.phone.borrow()[1], "north: landing-lock cleared | landing-lock cleared");
    assert_eq!(courier.proxy.borrow()[1], "[ops cleared] landing-lock");
    assert!(alarms.is_empty());
    check::sound(&[outcome("landing-lock", State::Holds)], &mut alarms, &courier, 1300);
    assert_eq!((courier.phone.borrow().len(), courier.proxy.borrow().len()), (2, 2));
}

#[test]
fn new_breaches_in_one_check_share_one_push_and_a_failed_push_retries_next_check() {
    let mut alarms = Alarms::new();
    let down = Recorder { down: true, ..Recorder::default() };
    let failed = check::sound(&[outcome("a", State::Breached), outcome("b", State::Breached)], &mut alarms, &down, 100);
    assert_eq!(failed.failures, ["phone: nexus unreachable"]);
    assert!(!alarms["a"].phoned && alarms["a"].told_proxy);
    let up = Recorder::default();
    check::sound(&[outcome("a", State::Breached), outcome("b", State::Breached)], &mut alarms, &up, 400);
    assert_eq!(up.phone.borrow().len(), 1);
    assert!(up.phone.borrow()[0].starts_with("north: 2 expectations breached | a BREACHED"));
    assert!(up.proxy.borrow().is_empty());
}

#[test]
fn an_expectation_removed_from_the_file_clears_its_alarm() {
    let courier = Recorder::default();
    let mut alarms = Alarms::new();
    check::sound(&[outcome("gone", State::Breached)], &mut alarms, &courier, 100);
    let report = check::sound(&[], &mut alarms, &courier, 400);
    assert_eq!(report.cleared, ["gone"]);
}

#[test]
fn the_cloud_list_reads_placement_time_from_created_at_when_present() {
    let listing = "session_a\trunning\tanthropic_cloud\t2026-10-16T09:46:16.884471Z\t2026-10-16T09:30:00Z\tNative client count\n\
                   session_b\tidle\tbridge\t2026-10-16T09:45:35Z\t2026-10-16T09:00:00Z\tPlato\n\
                   session_c\tidle\tanthropic_cloud\t2026-10-15T11:57:42.816515Z\tShowcase camera";
    let sessions = parse::cloud_sessions(listing);
    assert_eq!(sessions.len(), 2);
    assert_eq!((sessions[0].placed_at, sessions[0].title.as_str()), (parse::seconds("2026-10-16T09:30:00Z").unwrap(), "Native client count"));
    assert_eq!((sessions[1].placed_at, sessions[1].title.as_str()), (parse::seconds("2026-10-15T11:57:42Z").unwrap(), "Showcase camera"));
}

#[test]
fn landing_lock_files_and_queue_tickets_parse() {
    assert_eq!(parse::lock_holder("pid 2695609 in /home/tom/code/north/worktrees/tui\n"), Some((2695609, "/home/tom/code/north/worktrees/tui".into())));
    let ticket = parse::ticket("01792143691144521-2695609", "/home/tom/code/north/worktrees/x\tops\tabc\tops-observability\n").unwrap();
    assert_eq!((ticket.since, ticket.pid, ticket.branch.as_str(), ticket.session.as_deref()), (1792143691, 2695609, "ops", Some("ops-observability")));
    assert_eq!(parse::repo_of("/home/tom/code/smashcraft/worktrees/rel-42"), Some("smashcraft".into()));
    assert_eq!(parse::repo_of("/tmp/lock-order-live/ready"), None);
}

#[test]
fn commits_issue_references_and_dividend_lines_parse() {
    let body = b"tree t\nparent p\nauthor A <a@x> 1792140000 +0000\ncommitter C <c@x> 1792140100 +0000\n\nFix the arc (#42)\n\nRefs smashcraft#43, see https://x/#7\n";
    let batch = [format!("abc commit {}\n", body.len()).into_bytes(), body.to_vec(), b"\ndef missing\n".to_vec()].concat();
    let commits = parse::commit_batch(&batch);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits["abc"].0, 1792140000);
    assert_eq!(parse::issue_refs(&commits["abc"].1), [42, 43]);
    let issue = "## Dividend\nDividend: cloud placements per day (`north ops dividend cloud-lane`)\n**Dividend:** heartbeat age. Checked by `north ops dividend alerts`.\n**Dividend:** p90 minutes (target under 5).\n";
    assert_eq!(parse::dividend_ids(issue), ["cloud-lane", "alerts"]);
}

#[test]
fn a_session_stopped_on_a_rejected_limit_is_found_until_it_resumes() {
    let stopped = "{\"type\":\"assistant\"}\n{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"rejected\",\"resetsAt\":1792142400}}\n{\"type\":\"result\"}\n";
    let stop = parse::limit_stop(stopped, "rel-31", "claude:c").unwrap();
    assert_eq!((stop.session.as_str(), stop.resets_at), ("rel-31", 1792142400));
    let resumed = format!("{stopped}{{\"type\":\"system\",\"subtype\":\"init\"}}\n");
    assert!(parse::limit_stop(&resumed, "rel-31", "claude:c").is_none());
}

#[test]
fn a_brief_sizes_its_issue_from_its_eta() {
    let brief = "Item: smashcraft#42 (Public 0.1 release work)\nCategory: feature\nETA: 120 min (uncalibrated)\n";
    assert_eq!(parse::brief_size(brief), Some(("smashcraft#42".into(), Size::M)));
    assert_eq!(Size::for_eta_minutes(45), Size::S);
    assert_eq!(Size::for_eta_minutes(181), Size::L);
    assert_eq!(parse::brief_size("Item: an idea without an issue\nETA: 30 min"), None);
}

#[test]
fn the_page_leads_with_breaches_and_escapes_text() {
    let quiet = super::snapshot::assemble(&super::facts::Facts::default(), &[]);
    assert!(super::page::render(&quiet).contains("Every expectation holds."));
    let mut breached = super::snapshot::assemble(&super::facts::Facts::default(), &[]);
    let mut lock = outcome("landing-lock", State::Breached);
    lock.behavior = "<script>".into();
    breached.breached = vec![lock.id.clone()];
    breached.expectations = vec![outcome("alerts", State::Holds), lock];
    let page = super::page::render(&breached);
    assert!(page.contains("<p class=bad>1 expectation breached: landing-lock</p>"));
    assert!(page.find("landing-lock: &lt;script&gt;").unwrap() < page.find("alerts: alerts behaves").unwrap());
}
