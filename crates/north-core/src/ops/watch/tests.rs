use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use super::gather::{abandoned, capacity_view, cloud_rows, remote_hosts, repo_of};
use super::probes::{capacity, hosts, runtime_follow, waits, workers};
use super::shells::{candidates, repo_of_slug};
use super::*;

const NOW: u64 = 1_793_750_000;

fn keys(alerts: &[Alert]) -> Vec<&str> {
    alerts.iter().map(|alert| alert.key.as_str()).collect()
}

fn host(name: &str, local: bool, capacity: Option<CapacityView>) -> HostView {
    HostView { name: name.into(), local, down_since: None, capacity, waste: None }
}

fn ticket(owner: &str, waited: u64, p90: Option<u64>) -> Ticket {
    Ticket { owner: owner.into(), class: "heavy".into(), queued_since: NOW - waited, wait_p90_secs: p90 }
}

fn queue(tickets: Vec<Ticket>, pressure: f64, since: Option<u64>) -> CapacityView {
    CapacityView { queued: tickets, cpu_some_avg10: pressure, pressure_target: 25.0, pressure_since: since }
}

#[test]
fn capacity_raises_a_ticket_queued_past_five_minutes_on_any_host() {
    let context = WatchContext { now: NOW, hosts: vec![host("gex45", false, Some(queue(vec![ticket("w1", 400, None)], 0.0, None)))], ..WatchContext::default() };
    let alerts = capacity(&context);
    assert_eq!(keys(&alerts), ["work:capacity:gex45:queue:w1"]);
    assert_eq!(alerts[0].owner.as_deref(), Some("w1"));
}

#[test]
fn capacity_stays_quiet_for_a_ticket_inside_its_predicted_wait() {
    let tickets = vec![ticket("w1", 200, None), ticket("w2", 900, Some(1200))];
    let context = WatchContext { now: NOW, hosts: vec![host("whiterabbit", true, Some(queue(tickets, 3.0, None)))], ..WatchContext::default() };
    assert!(capacity(&context).is_empty());
}

#[test]
fn capacity_raises_laptop_pressure_held_two_minutes_while_work_queues() {
    let view = queue(vec![ticket("w1", 60, None)], 31.0, Some(NOW - 130));
    let context = WatchContext { now: NOW, hosts: vec![host("whiterabbit", true, Some(view))], ..WatchContext::default() };
    assert_eq!(keys(&capacity(&context)), ["work:capacity:whiterabbit:pressure"]);
}

#[test]
fn capacity_stays_quiet_for_brief_pressure_or_pressure_with_no_queue() {
    let brief = queue(vec![ticket("w1", 60, None)], 31.0, Some(NOW - 60));
    let idle = queue(Vec::new(), 31.0, Some(NOW - 600));
    let context = WatchContext { now: NOW, hosts: vec![host("whiterabbit", true, Some(brief)), host("whiterabbit", true, Some(idle))], ..WatchContext::default() };
    assert!(capacity(&context).is_empty());
}

fn working(name: &str, worked: u64, quiet: u64, eta: Option<u64>, waiting: bool) -> SessionView {
    SessionView { name: name.into(), status: "working".into(), parent: Some("lead".into()), repo: Some("north".into()), working_since: NOW - worked, last_event: Some(NOW - quiet), eta_min: eta, waiting }
}

#[test]
fn workers_raises_a_session_past_twice_its_eta_and_one_silent_ten_minutes() {
    let context = WatchContext { now: NOW, sessions: vec![working("late", 41 * 60, 30, Some(20), false), working("mute", 300, 11 * 60, Some(60), false)], ..WatchContext::default() };
    let alerts = workers(&context);
    assert_eq!(keys(&alerts), ["work:worker:late:overrun", "work:worker:mute:silent"]);
    assert_eq!(alerts[0].owner.as_deref(), Some("lead"));
}

#[test]
fn workers_stays_quiet_for_sessions_inside_eta_waiting_or_not_working() {
    let mut parked = working("parked", 10 * 3600, 3600, Some(20), false);
    parked.status = "parked".into();
    let context = WatchContext { now: NOW, sessions: vec![working("busy", 30 * 60, 60, Some(20), false), working("waiter", 600, 30 * 60, Some(60), true), parked], ..WatchContext::default() };
    assert!(workers(&context).is_empty());
}

#[test]
fn workers_raises_a_silent_running_cloud_session_and_a_subagent_nobody_woke() {
    let cloud = CloudView { sessions: vec![CloudSession { id: "cse_1".into(), status: "running".into(), updated: NOW - 900, title: "fix".into() }], failure: None };
    let agent = SubagentView { agent: "agent-a1".into(), session: "s1".into(), owner: Some("lead".into()), repo: Some("north".into()), last_event: NOW - 700, shell_finished: Some(NOW - 650) };
    let context = WatchContext { now: NOW, cloud, subagents: vec![agent], ..WatchContext::default() };
    assert_eq!(keys(&workers(&context)), ["work:cloud:cse_1:silent", "work:subagent:agent-a1:unwoken"]);
}

#[test]
fn workers_stays_quiet_for_an_active_cloud_session_and_a_subagent_still_moving() {
    let cloud = CloudView { sessions: vec![CloudSession { id: "cse_1".into(), status: "running".into(), updated: NOW - 60, title: "fix".into() }, CloudSession { id: "cse_2".into(), status: "idle".into(), updated: NOW - 9000, title: "done".into() }], failure: None };
    let agent = SubagentView { agent: "agent-a1".into(), session: "s1".into(), owner: None, repo: None, last_event: NOW - 120, shell_finished: Some(NOW - 60) };
    let context = WatchContext { now: NOW, cloud, subagents: vec![agent], ..WatchContext::default() };
    assert!(workers(&context).is_empty());
}

fn wait(run: &str, waited: u64) -> WaitView {
    WaitView { session: "w1".into(), run: run.into(), repo: Some("wisp".into()), since: NOW - waited }
}

#[test]
fn waits_raises_a_farm_run_past_its_expected_duration() {
    let context = WatchContext { now: NOW, waits: vec![wait("farm:123", 50 * 60), wait("job:capture@gex45", 61 * 60)], ..WatchContext::default() };
    assert_eq!(keys(&waits(&context)), ["work:wait:w1:farm:123", "work:wait:w1:job:capture@gex45"]);
}

#[test]
fn waits_stays_quiet_inside_the_expected_duration() {
    let context = WatchContext { now: NOW, waits: vec![wait("farm:123", 40 * 60), wait("job:x", 30 * 60)], ..WatchContext::default() };
    assert!(waits(&context).is_empty());
}

#[test]
fn runtime_follow_raises_four_abandons_in_half_an_hour_and_a_stale_runtime() {
    let runtime = RuntimeView { abandoned: vec![NOW - 1700, NOW - 1200, NOW - 600, NOW - 60], live: Some("3a200b133816898a".into()), rust_change_since: Some(NOW - 16 * 60) };
    let context = WatchContext { now: NOW, runtime, ..WatchContext::default() };
    assert_eq!(keys(&runtime_follow(&context)), ["work:runtime:abandoned", "work:runtime:stale"]);
}

#[test]
fn runtime_follow_stays_quiet_for_three_abandons_and_a_fresh_rust_change() {
    let runtime = RuntimeView { abandoned: vec![NOW - 4000, NOW - 1200, NOW - 600, NOW - 60], live: Some("3a200b13".into()), rust_change_since: Some(NOW - 10 * 60) };
    let context = WatchContext { now: NOW, runtime, ..WatchContext::default() };
    assert!(runtime_follow(&context).is_empty());
}

#[test]
fn hosts_pages_a_host_unreachable_for_two_minutes() {
    let mut down = host("gex45", false, None);
    down.down_since = Some(NOW - 125);
    let alerts = hosts(&WatchContext { now: NOW, hosts: vec![down], ..WatchContext::default() });
    assert_eq!(keys(&alerts), ["work:host:gex45:unreachable"]);
    assert_eq!(alerts[0].severity, Severity::Page);
}

#[test]
fn hosts_stays_quiet_for_a_reachable_host_or_a_brief_drop() {
    let mut blip = host("nexus", false, None);
    blip.down_since = Some(NOW - 30);
    assert!(hosts(&WatchContext { now: NOW, hosts: vec![blip, host("gex45", false, None)], ..WatchContext::default() }).is_empty());
}

#[test]
fn one_pass_runs_every_probe() {
    let mut down = host("gex45", false, Some(queue(vec![ticket("w1", 400, None)], 0.0, None)));
    down.down_since = Some(NOW - 300);
    let context = WatchContext {
        now: NOW,
        hosts: vec![down],
        sessions: vec![working("late", 100 * 60, 30, Some(20), false)],
        waits: vec![wait("farm:9", 3600)],
        runtime: RuntimeView { rust_change_since: Some(NOW - 3600), ..RuntimeView::default() },
        ..WatchContext::default()
    };
    let keys: Vec<String> = pass(&context).into_iter().map(|alert| alert.key).collect();
    assert_eq!(keys, ["work:capacity:gex45:queue:w1", "work:worker:late:overrun", "work:wait:w1:farm:9", "work:runtime:stale", "work:host:gex45:unreachable"]);
}

#[test]
fn gathered_sources_parse_into_the_snapshot() {
    let status = json!({"profile": "attended", "cpuSomeAvg10": 12.5, "queued": [{"owner": "w1", "class": "heavy", "waitingSeconds": 330.4}]});
    let view = capacity_view(&status, NOW);
    assert_eq!((view.queued[0].owner.as_str(), view.queued[0].queued_since, view.pressure_target), ("w1", NOW - 330, 25.0));
    let rows = cloud_rows("cse_1\trunning\tanthropic_cloud\t2026-11-04T00:00:00Z\tfix it\n");
    assert_eq!((rows[0].status.as_str(), rows[0].updated), ("running", 1_793_750_400));
    let hosts = remote_hosts("[hosts.nexus]\ndeploy_ssh = \"nexus\"\n[hosts.gex45]\nssh = \"tom@gex45\"\n[hosts.whiterabbit]\nresident_cap = 8\n", "whiterabbit");
    assert_eq!(hosts, [("nexus".to_owned(), "nexus".to_owned()), ("gex45".to_owned(), "tom@gex45".to_owned())]);
    assert_eq!(abandoned("1793749000 abc\n1793749100 def\n"), [1_793_749_000, 1_793_749_100]);
    assert_eq!(repo_of("gex45:/home/tom/code/smashcraft/main").as_deref(), Some("smashcraft"));
    assert_eq!(repo_of_slug("-home-tom-code-smashcraft-client-worktrees-x").as_deref(), Some("smashcraft-client"));
}

fn set_modified(path: &std::path::Path, at: SystemTime) {
    fs::File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
}

#[test]
fn a_subagent_quiet_after_its_shell_finished_is_a_candidate_and_one_woken_after_is_not() {
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("profiles/claude-a/projects/-home-tom-code-north-main/s1");
    let tasks = root.path().join("scratch/-home-tom-code-north-main/s1/tasks");
    fs::create_dir_all(session.join("subagents")).unwrap();
    fs::create_dir_all(&tasks).unwrap();
    let stuck = session.join("subagents/agent-stuck.jsonl");
    let woken = session.join("subagents/agent-woken.jsonl");
    fs::write(&stuck, "").unwrap();
    fs::write(&woken, "").unwrap();
    fs::write(tasks.join("b1.output"), "").unwrap();
    let start = SystemTime::now();
    set_modified(&stuck, start + Duration::from_secs(5));
    set_modified(&woken, start + Duration::from_secs(120));
    set_modified(&tasks.join("b1.output"), start + Duration::from_secs(60));
    let now = start.duration_since(UNIX_EPOCH).unwrap().as_secs() + 20 * 60;
    let found = candidates(&root.path().join("profiles"), &root.path().join("scratch"), now, 6 * 3600, 600);
    let agents: Vec<&str> = found.iter().map(|candidate| candidate.agent.as_str()).collect();
    assert_eq!(agents, ["agent-stuck"]);
    assert_eq!(found[0].repo.as_deref(), Some("north"));
}
