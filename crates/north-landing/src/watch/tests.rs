use std::cell::RefCell;
use std::collections::BTreeSet;

use serde_json::json;

use super::facts::{Event, RepoFacts, Run, StageSample, Ticket, failing_tests, parse_events, parse_stages, parse_ticket, slug_of};
use super::sink::{DESKTOP_SECONDS, Outbox, REPEAT_SECONDS, deliver, load};
use super::{Alert, Severity, WatchContext, evaluate};

const NOW: u64 = 1_793_750_000;

#[derive(Default)]
struct Recorder {
    sent: RefCell<Vec<(String, String)>>,
    desktop: RefCell<Vec<String>>,
}

impl Outbox for Recorder {
    fn send(&self, to: &str, text: &str) -> bool {
        self.sent.borrow_mut().push((to.to_owned(), text.to_owned()));
        true
    }
    fn notify(&self, text: &str) {
        self.desktop.borrow_mut().push(text.to_owned());
    }
    fn lead_of(&self, session: &str) -> Option<String> {
        (session == "worker-a").then(|| "lead-a".to_owned())
    }
}

fn event(pid: u32, lane: &str, stage: &str, status: &str, ago: f64) -> Event {
    Event { time: NOW as f64 - ago, repo: "north".into(), lane: lane.into(), sha: format!("{lane}-sha"), stage: stage.into(), status: status.into(), pid, session: "worker-a".into(), ..Event::default() }
}

fn context(events: Vec<Event>, live: &[u32]) -> WatchContext {
    WatchContext { now: NOW, events, live: live.iter().copied().collect::<BTreeSet<_>>(), ..WatchContext::default() }
}

fn keys(context: &WatchContext) -> Vec<String> {
    evaluate(context).into_iter().map(|alert| alert.key).collect()
}

#[test]
fn safe_push_event_lines_parse_with_their_fields() {
    let line = r#"{"time":1793749990.123456,"repo":"north","lane":"land-watch","sha":"abc","stage":"land checks","status":"wait","detail":"say \"hi\"","seconds":0,"pid":42,"session":"worker-a","host":"whiterabbit"}"#;
    let events = parse_events(&format!("{line}\nnot json\n"), NOW as f64);
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].stage.as_str(), events[0].status.as_str(), events[0].detail.as_str(), events[0].pid), ("land checks", "wait", "say \"hi\"", 42));
}

#[test]
fn a_landing_silent_for_120_seconds_outside_a_known_wait_pages_as_a_stall() {
    let stalled = context(vec![event(7, "lane-a", "landing lock", "start", 130.0)], &[7]);
    let alerts = evaluate(&stalled);
    assert_eq!(alerts[0].key, "land:north:stall:lane-a");
    assert_eq!(alerts[0].severity, Severity::Page);
    assert!(keys(&context(vec![event(7, "lane-a", "landing lock", "start", 100.0)], &[7])).is_empty());
    assert!(keys(&context(vec![event(7, "lane-a", "landing lock queue", "wait", 900.0)], &[7])).is_empty());
    assert!(keys(&context(vec![event(7, "lane-a", "landing lock", "start", 900.0)], &[])).is_empty());
    assert!(keys(&context(vec![event(7, "lane-a", "exit", "landed", 900.0)], &[7])).is_empty());
}

#[test]
fn a_check_past_its_p90_and_half_again_warns() {
    let mut overrun = context(vec![event(7, "lane-a", "land checks", "wait", 500.0)], &[7]);
    overrun.stages = [100.0, 120.0, 150.0, 200.0, 300.0].iter().map(|seconds| StageSample { repo: "north".into(), stage: "land checks".into(), seconds: *seconds, total: 0.0 }).collect();
    assert_eq!(keys(&overrun), ["land:north:overrun:lane-a"]);
    overrun.events[0].time = NOW as f64 - 400.0;
    assert!(keys(&overrun).is_empty());
}

#[test]
fn a_lane_requeued_more_than_twice_warns() {
    let mut retries: Vec<Event> = (0..3).map(|i| event(7, "lane-a", "pre-push checks", "retry", 300.0 - f64::from(i))).collect();
    retries.push(event(7, "lane-a", "pre-push checks", "wait", 10.0));
    assert_eq!(keys(&context(retries.clone(), &[7])), ["land:north:requeue:lane-a"]);
    assert!(keys(&context([&retries[..2], &retries[3..]].concat(), &[7])).is_empty());
}

#[test]
fn a_queue_without_a_landing_past_its_expected_time_pages() {
    let ticket = |pid, ago: f64| Ticket { queue: "abc.queue".into(), time: NOW as f64 - ago, pid, repo: "north".into(), lane: format!("lane-{pid}"), session: "worker-a".into() };
    let mut queue = context(vec![event(1, "lane-x", "exit", "landed", 500.0)], &[1, 2]);
    queue.tickets = vec![ticket(1, 900.0), ticket(2, 800.0)];
    queue.stages = vec![StageSample { repo: "north".into(), stage: "cleanup".into(), seconds: 1.0, total: 200.0 }];
    assert!(keys(&queue).is_empty());
    queue.tickets[0].time = NOW as f64 - 1000.0;
    queue.events.clear();
    assert_eq!(keys(&queue), ["land:north:queue-stalled"]);
    queue.stages[0].total = 600.0;
    assert!(keys(&queue).is_empty());
}

#[test]
fn a_github_push_rejection_warns_until_the_lane_lands() {
    let mut rejected = event(7, "lane-a", "push", "failed", 60.0);
    rejected.detail = "git push exited 1: remote: fatal error in commit_refs".into();
    let alerts = evaluate(&context(vec![rejected.clone()], &[]));
    assert_eq!(alerts[0].key, "land:north:push-error:lane-a");
    assert!(alerts[0].summary.contains("commit_refs"));
    assert!(keys(&context(vec![rejected, event(8, "lane-a", "exit", "landed", 10.0)], &[])).is_empty());
}

#[test]
fn two_lanes_failing_one_test_page_a_possible_red_main() {
    let failed = |lane: &str, ago| Event { tests: vec!["landing::tests::shared".into()], ..event(7, lane, "land check", "failed", ago) };
    assert_eq!(keys(&context(vec![failed("lane-a", 300.0), failed("lane-b", 200.0)], &[])), ["land:north:possible-red-main:landing::tests::shared"]);
    assert!(keys(&context(vec![failed("lane-a", 300.0), failed("lane-a", 200.0)], &[])).is_empty());
    assert!(keys(&context(vec![failed("lane-a", 5000.0), failed("lane-b", 200.0)], &[])).is_empty());
}

#[test]
fn failing_test_names_come_from_the_red_check_log_parser() {
    let log = "test watch::tests::ok ... ok\ntest watch::tests::bad ... FAILED\n---- watch::tests::bad stdout ----\nFAIL: hooks.test.sh\n";
    assert_eq!(failing_tests(log), ["watch::tests::bad", "hooks.test.sh"]);
}

fn run(name: &str, branch: &str, status: &str, conclusion: Option<&str>, ago: u64) -> Run {
    let at = time::OffsetDateTime::from_unix_timestamp((NOW - ago) as i64).unwrap();
    let stamp = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute(), at.second());
    assert_eq!(super::github::at(&stamp), Some(NOW - ago));
    Run { id: ago, name: name.into(), path: format!(".github/workflows/{}.yml", name.to_lowercase()), head_branch: branch.into(), head_sha: "f".repeat(40), event: "push".into(), status: status.into(), conclusion: conclusion.map(str::to_owned), created_at: stamp.clone(), updated_at: stamp, html_url: "https://github.com/x/runs/1".into() }
}

fn repo(runs: Vec<Run>) -> RepoFacts {
    RepoFacts { name: "north".into(), slug: "tompassarelli/north".into(), runs, ..RepoFacts::default() }
}

#[test]
fn main_ci_failing_on_a_main_commit_pages_and_names_lanes_that_failed_too() {
    let mut red = context(vec![event(7, "lane-a", "land check", "failed", 100.0)], &[]);
    red.repos = vec![repo(vec![run("Cargo", "main", "completed", Some("failure"), 200), run("Cargo", "main", "completed", Some("cancelled"), 50), run("Harness", "main", "completed", Some("success"), 60)])];
    let alerts = evaluate(&red);
    let alert = alerts.iter().find(|alert| alert.key == "gh:north:red-main:Cargo").unwrap();
    assert!(alert.summary.contains("land checks also failed in the last hour for lane-a"));
    red.repos[0].runs.push(run("Cargo", "main", "completed", Some("success"), 40));
    assert!(!keys(&red).iter().any(|key| key.contains("red-main")));
}

#[test]
fn an_actions_run_queued_over_120_seconds_warns() {
    let mut waiting = context(Vec::new(), &[]);
    waiting.repos = vec![repo(vec![run("Land check", "land-x", "queued", None, 150)])];
    assert_eq!(keys(&waiting), ["gh:north:queued"]);
    waiting.repos = vec![repo(vec![run("Land check", "land-x", "queued", None, 60)])];
    assert!(keys(&waiting).is_empty());
}

#[test]
fn autoland_failures_and_idle_land_branches_alert() {
    let mut autoland = context(Vec::new(), &[]);
    autoland.repos = vec![RepoFacts { autoland: true, land_branches: vec!["claude/land-a-0123".into()], ..repo(vec![run("Autoland", "claude/land-a-0123", "completed", Some("failure"), 900)]) }];
    assert_eq!(keys(&autoland), ["gh:north:autoland-failed:claude/land-a-0123", "gh:north:autoland-stall"]);
    autoland.repos[0].runs.push(run("Autoland", "claude/land-a-0123", "in_progress", None, 10));
    assert!(keys(&autoland).is_empty());
}

#[test]
fn an_unreadable_github_keeps_its_incidents_and_warns() {
    let mut broken = context(Vec::new(), &[]);
    broken.repos = vec![RepoFacts { error: Some("gh api: HTTP 502".into()), ..repo(Vec::new()) }];
    assert_eq!(keys(&broken), ["gh:north:api-error"]);
}

#[test]
fn a_runtime_not_live_after_a_landing_goes_to_the_landing_lane_owner() {
    let mut settle = context(vec![Event { sha: "a".repeat(40), ..event(7, "lane-a", "exit", "landed", 900.0) }], &[]);
    settle.settle = Some(json!({"checkedAt": NOW - 30, "landing": "a".repeat(40), "hosts": [{"host": "nexus", "problems": ["runtime behind"]}, {"host": "whiterabbit", "problems": []}]}));
    let alerts = evaluate(&settle);
    assert_eq!(alerts[0].key, format!("land:north:runtime-not-live:{}", "a".repeat(12)));
    assert_eq!(alerts[0].owner.as_deref(), Some("worker-a"));
    settle.settle = Some(json!({"checkedAt": NOW - 30, "landing": "a".repeat(40), "hosts": [{"host": "nexus", "problems": []}]}));
    assert!(keys(&settle).is_empty());
}

fn page(since: u64) -> Alert {
    Alert { key: "land:north:stall:lane-a".into(), severity: Severity::Page, repo: Some("north".into()), owner: Some("worker-a".into()), summary: "north lane lane-a stalled".into(), since }
}

#[test]
fn the_sink_sends_once_repeats_every_ten_minutes_notifies_once_and_clears() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incidents.json");
    let outbox = Recorder::default();
    deliver(&path, &[page(NOW)], &[], NOW, &outbox);
    assert_eq!(*outbox.sent.borrow(), [("proxy".to_owned(), "north watch: north lane lane-a stalled".to_owned()), ("lead-a".to_owned(), "north watch: north lane lane-a stalled".to_owned())]);
    deliver(&path, &[page(NOW)], &[], NOW + 30, &outbox);
    assert_eq!(outbox.sent.borrow().len(), 2);
    assert!(outbox.desktop.borrow().is_empty());
    deliver(&path, &[page(NOW)], &[], NOW + DESKTOP_SECONDS, &outbox);
    deliver(&path, &[page(NOW)], &[], NOW + DESKTOP_SECONDS + 30, &outbox);
    assert_eq!(outbox.desktop.borrow().len(), 1);
    deliver(&path, &[page(NOW)], &[], NOW + REPEAT_SECONDS, &outbox);
    assert_eq!(outbox.sent.borrow().len(), 4);
    assert!(outbox.sent.borrow()[3].1.starts_with("north watch: still, 10 min:"));
    deliver(&path, &[], &["land:".to_owned()], NOW + REPEAT_SECONDS + 30, &outbox);
    assert_eq!(load(&path).len(), 1);
    deliver(&path, &[], &[], NOW + REPEAT_SECONDS + 60, &outbox);
    assert!(outbox.sent.borrow()[5].1.starts_with("north watch: cleared after 11 min:"));
    assert!(load(&path).is_empty());
}

#[test]
fn queue_tickets_and_stage_rows_parse() {
    let ticket = parse_ticket("k.queue", "01793749999123456-4242", "/home/tom/code/north/worktrees/land-watch\tland-watch\tabc\tworker-a\n").unwrap();
    assert_eq!((ticket.repo.as_str(), ticket.lane.as_str(), ticket.session.as_str(), ticket.pid), ("north", "land-watch", "worker-a", 4242));
    assert!(parse_ticket("k.queue", "1793749999123456-4242.result", "").is_none());
    assert_eq!(parse_stages("1793749970\tnorth\ttrain\tlanguage gate\t1058\t3012\n")[0].seconds, 1.058);
    assert_eq!(slug_of("https://github.com/tompassarelli/firn.git").as_deref(), Some("tompassarelli/firn"));
}
