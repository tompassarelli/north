use std::cell::RefCell;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{ExitStatus, Output};
use std::time::Duration;

use serde_json::{Value, json};

use super::alerts::{Node, State, alert_plan, stall_alert};
use super::host::{Host, Places, iso, seconds, with_user_bin};
use super::jobs::landing_jobs;
use super::measure::{Env, get};
use super::stall::{Facts, Job, Medians, WorkflowMedians, job_medians, landing_run, verdict, workflow_median};

pub(super) struct Fake<'a>(pub(super) &'a dyn Fn(&[&str]) -> Option<String>);

impl Host for Fake<'_> {
    fn output(&self, argv: &[&str], _: Option<&Path>, _: Option<&str>, _: Duration) -> Option<Output> {
        let said = (self.0)(argv);
        Some(Output { status: ExitStatus::from_raw(if said.is_some() { 0 } else { 256 }), stdout: said.unwrap_or_default().into_bytes(), stderr: Vec::new() })
    }
}

pub(super) fn places(directory: &Path) -> Places {
    Places { home: directory.to_path_buf(), state: directory.to_path_buf(), ledger: directory.join("threads.db") }
}

pub(super) fn at(text: &str) -> f64 {
    seconds(text).unwrap()
}

fn facts<'a>(now: f64, main_at: f64, landings: &'a [f64], pending: u64, jobs: &'a [Job], medians: &'a Medians, since: Option<f64>) -> Facts<'a> {
    Facts { now, main_at, landings, pending, jobs, medians, pending_since: since }
}

struct Recorded {
    runs: Vec<Value>,
    jobs: Vec<(Job, Option<f64>)>,
    landings: Vec<f64>,
    windows: Value,
}

fn recorded() -> Recorded {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/smashcraft-2026-10-11.json")).unwrap();
    let named = |fields: &Value, values: &Value| -> Value {
        let mut item: serde_json::Map<String, Value> = fields.as_array().unwrap().iter().zip(values.as_array().unwrap()).map(|(field, value)| (field.as_str().unwrap().to_owned(), value.clone())).collect();
        item.insert("workflowName".into(), "Autoland".into());
        Value::Object(item)
    };
    Recorded {
        runs: fixture["runs"].as_array().unwrap().iter().map(|values| named(&fixture["run_fields"], values)).collect(),
        jobs: fixture["jobs"].as_array().unwrap().iter().map(|values| job_of(&fixture["job_fields"], values)).collect(),
        landings: fixture["landings"].as_array().unwrap().iter().map(|at| at.as_f64().unwrap()).collect(),
        windows: fixture["windows"].clone(),
    }
}

fn job_of(fields: &Value, values: &Value) -> (Job, Option<f64>) {
    let raw: serde_json::Map<String, Value> = fields.as_array().unwrap().iter().zip(values.as_array().unwrap()).map(|(field, value)| (field.as_str().unwrap().to_owned(), value.clone())).collect();
    let runner = raw["runner"].as_str().filter(|runner| !runner.is_empty()).unwrap_or("waiting");
    let done = raw["completed_at"].as_str().filter(|done| !done.is_empty()).map(at);
    let job = Job { workflow: "autoland.yml".into(), run_id: raw["run_id"].as_u64().unwrap(), name: raw["name"].as_str().unwrap().into(), started_at: at(raw["started_at"].as_str().unwrap()), runner: runner.into() };
    (job, done)
}

fn row(recorded: &Recorded, now: f64) -> Value {
    let landings: Vec<f64> = recorded.landings.iter().copied().filter(|landed| *landed <= now).collect();
    let active: Vec<Job> = recorded.jobs.iter().filter(|(job, done)| job.started_at <= now && done.is_none_or(|done| now < done)).map(|(job, _)| job.clone()).collect();
    let waiting: Vec<f64> = recorded
        .runs
        .iter()
        .filter(|item| landing_run(item) && at(item["createdAt"].as_str().unwrap()) <= now)
        .filter(|item| item["conclusion"].as_str().unwrap_or_default().is_empty() || now < at(item["updatedAt"].as_str().unwrap()))
        .map(|item| at(item["createdAt"].as_str().unwrap()))
        .collect();
    let latest = landings.iter().copied().fold(f64::MIN, f64::max);
    let medians = Medians::from([("autoland.yml".to_owned(), WorkflowMedians { run: workflow_median(&recorded.runs, now), ..Default::default() })]);
    let since = waiting.iter().copied().min_by(f64::total_cmp);
    json!({ "repo": "smashcraft", "main_sha": (latest as i64).to_string(), "stall": verdict(&facts(now, latest, &landings, waiting.len() as u64, &active, &medians, since)) })
}

fn parent() -> Vec<Node> {
    vec![Node { id: "smashcraft-parent".into(), parent: None, active: true, cwd: "/home/tom/code/smashcraft/main".into() }]
}

#[test]
fn test_spawn_recording_flow_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let db = rusqlite::Connection::open(&places.ledger).unwrap();
    db.execute("CREATE TABLE runs(role TEXT,outcome TEXT,category TEXT,spec TEXT,scope TEXT,surface TEXT,verify TEXT,brief_text TEXT,brief_origin TEXT)", []).unwrap();
    let brief = "Category: tooling\nSpec: measured\nScope: module\nSurface: shell\nVerify: native";
    let origin = "codex.PostToolUse.spawn_agent";
    let rows = [
        ("done", Some("measured"), Some(brief), Some(origin)),
        ("running", Some("measured"), Some(brief), Some(origin)),
        ("done", Some("measured"), None, Some(origin)),
        ("done", None, Some(brief), Some(origin)),
        ("done", Some("measured"), Some(brief), Some("claude.PostToolUse.Agent")),
        ("running", Some("measured"), Some(brief), Some("claude.PostToolUse.Agent")),
    ];
    for (outcome, spec, text, from) in rows {
        db.execute("INSERT INTO runs VALUES('worker',?1,'tooling',?2,'module','shell','native',?3,?4)", rusqlite::params![outcome, spec, text, from]).unwrap();
    }
    assert_eq!(super::notes::brief_coverage(&places), Some(2));
}

#[test]
fn test_recorded_11_oct_landing_window() {
    let recorded = recorded();
    replay_alerts(&recorded);
    deliver_alerts(&recorded);
}

fn replay_alerts(recorded: &Recorded) {
    let nodes = parent();
    let mut events: std::collections::BTreeMap<String, Vec<(f64, super::alerts::Action)>> = Default::default();
    for (window, bounds) in recorded.windows.as_object().unwrap() {
        let (mut state, mut found) = (State::new(), Vec::new());
        let mut now = at(bounds[0].as_str().unwrap());
        while now <= at(bounds[1].as_str().unwrap()) {
            let (next, actions) = alert_plan(&[row(recorded, now)], &state, now, &nodes);
            state = next;
            for action in actions {
                state.get_mut(&action.repo).unwrap().sent.insert(action.key.clone(), now);
                found.push((now, action));
            }
            now += 300.0;
        }
        events.insert(window.clone(), found);
    }
    assert!(events["healthy"].is_empty());
    let first = events["stall"].iter().map(|(at, _)| *at).fold(f64::MAX, f64::min);
    assert!((at("2026-10-11T13:36:00Z")..=at("2026-10-11T14:10:00Z")).contains(&first), "{first}");
    let mut recipients: Vec<(&str, &str)> = events["stall"].iter().filter(|(at, _)| *at == first).map(|(_, action)| (action.channel, action.node.as_str())).collect();
    recipients.sort_unstable();
    assert_eq!(recipients, [("fleet", "proxy"), ("fleet", "smashcraft-parent")]);
    assert_eq!(row(recorded, first)["stall"]["runner"], "vast-smashcraft-8");
}

fn deliver_alerts(recorded: &Recorded) {
    let first = events_first(recorded);
    let stalled = row(recorded, first);
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let sends = RefCell::new(Vec::<String>::new());
    let fleet = json!([{"name": "smashcraft-parent", "conversation": "smashcraft-session", "parent": null, "provider": "claude", "status": "working", "cwd": "/home/tom/code/smashcraft/main"}]).to_string();
    let reply = |argv: &[&str]| match argv {
        ["north", "fleet", "list", "--json"] => Some(fleet.clone()),
        ["north", "fleet", "send", node, text] => {
            sends.borrow_mut().push(format!("fleet send {node} {text}"));
            Some(String::new())
        }
        _ => None,
    };
    let host = Fake(&reply);
    let state_path = directory.path().join("agents/flow/stalls.json");
    stall_alert(&places, &host, std::slice::from_ref(&stalled), first).unwrap();
    stall_alert(&places, &host, std::slice::from_ref(&stalled), first + 300.0).unwrap();
    stall_alert(&places, &host, std::slice::from_ref(&stalled), first + 600.0).unwrap();
    assert_eq!(sends.borrow().len(), 2);
    stall_alert(&places, &host, std::slice::from_ref(&stalled), first + 900.0).unwrap();
    assert_eq!(sends.borrow().len(), 4);
    stall_alert(&places, &host, std::slice::from_ref(&stalled), first + 1200.0).unwrap();
    assert_eq!(sends.borrow().len(), 4);
    assert!(sends.borrow().iter().any(|message| message.starts_with("fleet send proxy ")));
    assert!(sends.borrow().iter().any(|message| message.starts_with("fleet send smashcraft-parent ")));
    let initial = std::fs::read_to_string(&state_path).unwrap();
    stall_alert(&places, &host, &[json!({"repo": "smashcraft", "stall": "error"})], first + 600.0).unwrap();
    assert_eq!(std::fs::read_to_string(&state_path).unwrap(), initial);

    let prolonged = first + 3600.0;
    let late = recorded.jobs.iter().map(|(job, _)| job).find(|job| job.run_id == 38_143_883_634 && job.name == "reports").unwrap();
    let medians = Medians::from([("autoland.yml".to_owned(), WorkflowMedians { run: workflow_median(&recorded.runs, first), ..Default::default() })]);
    let fault = json!({"repo": "smashcraft", "main_sha": "moved", "stall": verdict(&facts(prolonged, prolonged, &[prolonged], 0, std::slice::from_ref(late), &medians, None))});
    assert_eq!(fault["stall"]["stalled"], true);
    assert_eq!(fault["stall"]["overdue_jobs"].as_array().unwrap().len(), 1);
    stall_alert(&places, &host, std::slice::from_ref(&fault), prolonged).unwrap();
    stall_alert(&places, &host, std::slice::from_ref(&fault), prolonged + 300.0).unwrap();
    assert_eq!(sends.borrow().len(), 6);
    let healthy = json!({"repo": "smashcraft", "main_sha": stalled["main_sha"], "stall": verdict(&facts(prolonged, prolonged, &[prolonged], 0, &[], &Medians::new(), None))});
    stall_alert(&places, &host, std::slice::from_ref(&healthy), prolonged).unwrap();
    assert_eq!(std::fs::read_to_string(&state_path).unwrap().trim(), "{}");
    stall_alert(&places, &host, std::slice::from_ref(&stalled), prolonged + 300.0).unwrap();
    assert_eq!(sends.borrow().len(), 8);
}

fn events_first(recorded: &Recorded) -> f64 {
    let stall = &recorded.windows["stall"];
    let mut now = at(stall[0].as_str().unwrap());
    loop {
        if row(recorded, now)["stall"]["stalled"] == true {
            return now;
        }
        now += 300.0;
    }
}

#[test]
fn a_lane_arriving_after_a_quiet_spell_is_not_a_stall() {
    let now = 1_793_000_000.0;
    let quiet_since = now - 215.0 * 60.0;
    let landings = [quiet_since - 600.0, quiet_since];
    let job = Job { workflow: "autoland.yml".into(), run_id: 1, name: "build".into(), started_at: now - 33.0, runner: "GitHub Actions 1".into() };
    let medians = Medians::from([("autoland.yml".to_owned(), WorkflowMedians { run: Some(10.0), ..Default::default() })]);
    let jobs = std::slice::from_ref(&job);
    let arrived = verdict(&facts(now, quiet_since, &landings, 2, jobs, &medians, Some(now - 120.0)));
    assert!(!arrived.stalled, "{}", arrived.reason);
    assert!((arrived.oldest_pending_minutes.unwrap() - 2.0).abs() < 1e-9);
    let waiting = verdict(&facts(now, quiet_since, &landings, 2, jobs, &medians, Some(now - 45.0 * 60.0)));
    assert!(waiting.stalled);
    assert!(waiting.reason.contains("oldest pending lane waiting 45.0m and main idle 215.0m"), "{}", waiting.reason);
    let recent = verdict(&facts(now, now - 600.0, &landings, 2, &[], &medians, Some(now - 45.0 * 60.0)));
    assert!(!recent.stalled, "{}", recent.reason);
    assert!(!verdict(&facts(now, quiet_since, &landings, 0, &[], &medians, None)).stalled);
    let slow = Job { started_at: now - 25.0 * 60.0, ..job };
    assert!(verdict(&facts(now, now, &landings, 0, std::slice::from_ref(&slow), &medians, None)).stalled);
}

#[test]
fn a_cached_row_is_recomputed_once_main_has_moved() {
    let now = 1_793_000_000.0;
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let cached = json!({"repo": "smashcraft", "epoch": now - 60.0, "target": null, "main_sha": "old", "reuse": null, "closed_per_hour": 0});
    std::fs::create_dir_all(places.flow()).unwrap();
    std::fs::write(places.flow().join("smashcraft.json"), cached.to_string()).unwrap();
    let tip = RefCell::new("old\n");
    let reply = |argv: &[&str]| (argv == ["git", "rev-parse", "origin/main"]).then(|| (*tip.borrow()).to_owned());
    let host = Fake(&reply);
    let env = Env::new(&places, &host, Default::default());
    assert_eq!(get(&env, "smashcraft", now, &["reuse", "closed_per_hour"])["main_sha"], "old");
    *tip.borrow_mut() = "new\n";
    let fresh = get(&env, "smashcraft", now, &["reuse", "closed_per_hour"]);
    assert_eq!(fresh["epoch"], now);
    assert_eq!(fresh["stall"], "error");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(places.flow().join("smashcraft.json")).unwrap()).unwrap();
    assert_eq!(saved["epoch"], now);
}

#[test]
fn a_unit_path_without_the_user_bin_gains_it_so_ci_diagnose_finds_claude() {
    let path = with_user_bin(Some("/run/current-system/sw/bin".into()), "/home/fixture".as_ref());
    assert_eq!(path, "/run/current-system/sw/bin:/home/fixture/.local/bin");
    assert_eq!(with_user_bin(Some(path.clone()), "/home/fixture".as_ref()), path);
    assert_eq!(with_user_bin(None, "/home/fixture".as_ref()), "/home/fixture/.local/bin");
}

#[test]
fn a_workflow_with_hundreds_of_runs_a_day_gets_its_median_from_one_listing_page_per_day() {
    let now = 1_793_000_000.0;
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let reply = |argv: &[&str]| -> Option<String> {
        match argv {
            ["git", "config", .., "ci.budget"] => Some("farm-test.yml 600\n".into()),
            ["git", "ls-tree", ..] => Some(String::new()),
            ["gh", "run", "list", .., "--status", "in_progress", _, _, _, _] => Some("[]".into()),
            ["gh", "run", "list", .., "--created", bounds, "--limit", limit, _, _] => {
                let (from, to) = bounds.split_once("..")?;
                let count: u64 = limit.parse().ok().filter(|limit| *limit <= 100)?;
                let first = (at(from) as u64) * 10;
                let runs: Vec<Value> = (0..count).map(|i| json!({"databaseId": first + i, "workflowName": "Farm test", "displayTitle": "farm", "conclusion": "success", "startedAt": iso(at(to) - 600.0), "updatedAt": iso(at(to) - 300.0)})).collect();
                Some(Value::from(runs).to_string())
            }
            _ => None,
        }
    };
    let host = Fake(&reply);
    let (jobs, medians) = landing_jobs(&places, &host, directory.path(), "owner/repo", now).expect("each listing fits one page, inside the per-command limit");
    assert!(jobs.is_empty());
    assert_eq!(medians["farm-test.yml"].run, Some(5.0));
}

fn rollback(minutes: f64, executed: bool) -> Vec<Value> {
    let step = |conclusion: &str| json!({"conclusion": conclusion});
    let steps: Vec<Value> = if executed { vec![step("success"); 6] } else { [vec![step("success"); 3], vec![step("skipped"); 3]].concat() };
    vec![json!({"name": "rollback", "conclusion": "success", "started_at": iso(0.0), "completed_at": iso(minutes * 60.0), "steps": steps})]
}

#[test]
fn a_job_is_held_to_twice_its_own_executed_median_not_the_cache_hit_heavy_run_median() {
    let now = 1_793_000_000.0;
    let history: Vec<Vec<Value>> = (0..8).map(|_| rollback(0.5, false)).chain([4.6, 4.4, 4.8].map(|minutes| rollback(minutes, true))).collect();
    let jobs = job_medians(&history);
    assert!((jobs["rollback"] - 4.6).abs() < 1e-9, "{jobs:?}");
    let medians = Medians::from([("farm-test.yml".to_owned(), WorkflowMedians { run: Some(1.8), jobs })]);
    let running = |minutes: f64, name: &str| Job { workflow: "farm-test.yml".into(), run_id: 1, name: name.into(), started_at: now - minutes * 60.0, runner: "gex45".into() };
    let overdue = |job: Job| verdict(&facts(now, now, &[now], 0, &[job], &medians, None)).overdue_jobs;
    assert!(overdue(running(5.0, "rollback")).is_empty());
    let late = overdue(running(10.0, "rollback"));
    assert_eq!(late.len(), 1);
    assert!((late[0].limit_minutes - 9.2).abs() < 1e-9);
    assert_eq!(overdue(running(4.0, "unseen")).len(), 1, "a job without history falls back to twice the workflow median");
}

#[test]
fn test_refused_lane_reaches_proxy_only_when_its_pusher_is_gone() {
    let sha = "a".repeat(40);
    let tips = std::collections::BTreeMap::from([(sha.clone(), (0.0, "claude/land-x-aaaa".to_owned()))]);
    let states = std::collections::BTreeMap::from([(sha.clone(), json!({"state": "FAILURE", "description": "rebasing onto main conflicts in a.rs", "targetUrl": "https://example.test/run", "createdAt": "2026-10-30T18:00:00Z"}))]);
    let dead = std::collections::BTreeSet::from([&sha]);
    let pushers = std::collections::BTreeMap::from([(sha.clone(), "wisp-114".to_owned())]);
    let now = at("2026-10-30T18:40:00Z");
    let row = json!({"repo": "wisp", "refused": super::refused::rows(&tips, &states, &dead, &pushers, now)});
    let node = |active| vec![Node { id: "wisp-114".into(), parent: Some("wisp-lead".into()), active, cwd: "/home/tom/code/wisp/main".into() }];
    let (_, live) = alert_plan(std::slice::from_ref(&row), &State::new(), now, &node(true));
    assert!(live.is_empty());
    let (_, gone) = alert_plan(std::slice::from_ref(&row), &State::new(), now, &node(false));
    let mut recipients: Vec<&str> = gone.iter().map(|action| action.node.as_str()).collect();
    recipients.sort_unstable();
    assert_eq!(recipients, ["proxy", "wisp-parent"]);
    assert_eq!(gone[0].text, "Land branches Autoland refused in wisp, with no live session to fix them: claude/land-x-aaaa (pushed by wisp-114, rebasing onto main conflicts in a.rs, https://example.test/run, 40m)");
}
