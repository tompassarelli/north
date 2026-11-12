use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use serde_json::{Value, json};

use crate::collect;
use crate::proc::{now, run};
use crate::scratch;

pub const PINNED_MODEL: &str = "claude-opus-5-5";
pub const EFFORT: &str = "medium";
pub const NUDGE: &str = "Continue with your best judgment; the task text is complete.";
const LIMIT_S: f64 = 2700.0;
const SETTLED_S: f64 = 45.0;
const POLL_S: u64 = 5;

pub struct Plan {
    pub level: usize,
    pub tasks: Vec<Value>,
    pub account: String,
    pub parent: String,
    pub hosts: Vec<String>,
    pub work: PathBuf,
    pub burst: Option<(usize, f64)>,
    pub one_message: bool,
    pub model: String,
}

pub struct Run {
    pub task: Value,
    pub branch: String,
    pub name: String,
    pub launch_start: f64,
    pub launch_end: f64,
    pub launch_exit: i32,
    pub launch_output: String,
    pub nudges: Vec<Value>,
    pub resolved: Option<(f64, String)>,
    pub settled_since: Option<f64>,
    pub load_at_launch: Value,
    pub wave: &'static str,
}

fn header(model: &str) -> String {
    format!("Category: feature\nSpec: exact\nScope: module\nSurface: shell\nVerify: local-test\nNeeds: cpx62\nModel: {model} at medium effort, pinned so Eval B holds the model fixed as Eval A does.\nDone when: the acceptance command below exits 0 and the change is on main, handed in from this lane the way your worker instructions say.\n\n")
}

pub fn prompt(task: &Value) -> String {
    format!("{}\n\nThe change is done when this acceptance command passes, run from the repository root: {}\n", task["text"].as_str().unwrap_or_default(), task["check"].as_str().unwrap_or_default())
}

const ONE_MESSAGE: &str = "Everything needed is above. In ONE message, write the files with one shell command, a heredoc per file with its whole contents, since the client refuses Edit and Write on a file it has not read, and call the finish tool in that same message. Read nothing and run nothing first. The finish call runs the acceptance check and returns the failure if there is one.";

fn one_message(task: &Value, cwd: &str) -> String {
    let shown: String = task["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|file| format!("Current contents of {cwd}/{file}:\n```\n{}```\n\n", fs::read_to_string(Path::new(cwd).join(file)).unwrap_or_default()))
        .collect();
    format!("{shown}{ONE_MESSAGE}\n")
}

fn launch(plan: &Plan, task: &Value, branch: &str, name: &str, wave: &'static str) -> Run {
    let cwd = scratch::worktree(branch);
    let text = header(&plan.model) + &prompt(task) + &if plan.one_message { format!("\n{}", one_message(task, &cwd)) } else { String::new() };
    let tier_reason = format!("Eval B pins {} at medium for the whole ladder", plan.model);
    let args = [
        "fleet", "launch", "--provider", "claude", "--account", &plan.account, "--model", &plan.model, "--effort", EFFORT, "--name", name, "--parent", &plan.parent, "--role", "worker", "--prompt", "north",
        "--tier-reason", &tier_reason, "--cwd", &cwd, "--no-wait", &text,
    ];
    let launch_start = now();
    let ran = run("north", &args, None);
    Run {
        task: task.clone(),
        branch: branch.to_owned(),
        name: name.to_owned(),
        launch_start,
        launch_end: now(),
        launch_exit: ran.code,
        launch_output: format!("{}{}", ran.out, ran.err),
        nudges: Vec::new(),
        resolved: if ran.ok() { None } else { Some((now(), "launch-refused".to_owned())) },
        settled_since: None,
        load_at_launch: crate::proc::host_loads(&plan.hosts),
        wave,
    }
}

fn statuses(names: &[&str]) -> BTreeMap<String, String> {
    let listed: Value = serde_json::from_str(&run("north", &["fleet", "list", "--json", "--all"], None).out).unwrap_or(Value::Null);
    listed.as_array().into_iter().flatten().filter(|s| names.contains(&s["name"].as_str().unwrap_or_default())).map(|s| (s["name"].as_str().unwrap_or_default().to_owned(), s["status"].as_str().unwrap_or_default().to_owned())).collect()
}

fn landed_branches(queue: Option<&Path>) -> BTreeMap<String, f64> {
    let Some(text) = queue.and_then(|q| fs::read_to_string(q.join("logs/spans.jsonl")).ok()) else { return BTreeMap::new() };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|span| span["category"] == "landing" && span["attributes"]["landed"].is_string())
        .filter_map(|span| Some((span["name"].as_str()?.to_owned(), span["end"].as_f64()?)))
        .collect()
}

fn step(run: &mut Run, status: Option<&String>, landed: Option<f64>, at: f64) {
    if let Some(end) = landed {
        run.resolved = Some((end, "landed".to_owned()));
        return;
    }
    if at - run.launch_start > LIMIT_S {
        run.resolved = Some((at, "time-limit".to_owned()));
        return;
    }
    let busy = status.is_some_and(|s| s == "working" || s == "starting");
    if busy {
        run.settled_since = None;
        return;
    }
    let since = *run.settled_since.get_or_insert(at);
    if at - since < SETTLED_S {
        return;
    }
    if run.nudges.len() >= 2 {
        run.resolved = Some((at, format!("third-stop-without-landing ({})", status.map_or("unlisted", String::as_str))));
        return;
    }
    run.nudges.push(json!({"at_s": (since - run.launch_start).round(), "stall_s": (at - since).round(), "status": status}));
    let _ = crate::proc::run("north", &["fleet", "send", &run.name, NUDGE], None);
    run.settled_since = None;
}

pub fn drive(plan: &Plan, stamp: &str) -> Vec<Run> {
    let mut pending: Vec<&Value> = plan.tasks.iter().rev().collect();
    let mut active: Vec<Run> = Vec::new();
    let mut done: Vec<Run> = Vec::new();
    let mut collectors = Vec::new();
    let began = now();
    loop {
        let held = plan.burst.filter(|&(_, after)| now() - began < after);
        let free = held.map_or(plan.level.saturating_sub(active.len()), |(initial, _)| initial.saturating_sub(active.len() + done.len()));
        let wave = match (plan.burst, held) {
            (None, _) => "",
            (Some(_), Some(_)) => "in-flight",
            (Some(_), None) => "burst",
        };
        let batch: Vec<&Value> = (0..free).filter_map(|_| pending.pop()).collect();
        let launched: Vec<Run> = thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|task| {
                    let id = task["id"].as_str().unwrap_or_default();
                    let branch = format!("{id}-l{}", plan.level);
                    let name = format!("evalb-l{}-{id}-{stamp}", plan.level);
                    scope.spawn(move || launch(plan, task, &branch, &name, wave))
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for run in &launched {
            let _ = fs::create_dir_all(plan.work.join(&run.name));
            let _ = fs::write(plan.work.join(&run.name).join("launch.log"), &run.launch_output);
            eprintln!("eval-b: launched {} (exit {}, {:.0} s)", run.name, run.launch_exit, run.launch_end - run.launch_start);
        }
        active.extend(launched);
        if active.is_empty() && pending.is_empty() {
            break;
        }
        thread::sleep(std::time::Duration::from_secs(POLL_S));
        let names: Vec<&str> = active.iter().map(|r| r.name.as_str()).collect();
        let status = statuses(&names);
        let landed = landed_branches(scratch::queue_dir().as_deref());
        let at = now();
        for run in active.iter_mut().filter(|r| r.resolved.is_none()) {
            step(run, status.get(&run.name), landed.get(&run.branch).copied().filter(|end| *end >= run.launch_start), at);
        }
        let (resolved, still): (Vec<Run>, Vec<Run>) = active.into_iter().partition(|r| r.resolved.is_some());
        active = still;
        for run in resolved {
            eprintln!("eval-b: {} resolved: {}", run.name, run.resolved.as_ref().map_or("", |r| r.1.as_str()));
            let work = plan.work.clone();
            let name = run.name.clone();
            let hosts = plan.hosts.clone();
            collectors.push(thread::spawn(move || {
                let _ = fs::write(work.join(&name).join("load-at-end.json"), crate::proc::host_loads(&hosts).to_string());
                collect::session(&work, &name)
            }));
            done.push(run);
        }
    }
    for handle in collectors {
        let _ = handle.join();
    }
    done
}
