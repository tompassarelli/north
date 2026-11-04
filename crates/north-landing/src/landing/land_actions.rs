use crate::platform::process;
use serde_json::{Value, json};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage: north land-actions --workflow FILE --base BASE --head HEAD --timeout SECONDS -- COMMAND...";
const GH_LIMIT: Duration = Duration::from_secs(60);
const DISPATCH_SECONDS: u64 = 180;
const QUEUED_SECONDS: u64 = 600;
const FAILED_LOG_LINES: usize = 80;

struct Request {
    workflow: String,
    base: String,
    head: String,
    timeout: Duration,
    commands: Vec<String>,
}

enum Verdict {
    Pending,
    Passed,
    Failed(String),
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(request) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let branch = format!("land-check/{}", request.head);
    match push_land_ref(&format!("+{}:refs/heads/{branch}", request.head), push_capturing, || thread::sleep(Duration::from_secs(5))) {
        RefPush::Pushed => {}
        RefPush::Failed => {
            eprintln!("land check: could not push {} to origin {branch}", request.head);
            return 1;
        }
        RefPush::Infrastructure => {
            eprintln!("land check: GitHub rejected the push of origin {branch} twice; infrastructure, not a check failure");
            return INFRASTRUCTURE;
        }
    }
    let status = check(&request, &branch);
    if !push(&format!(":refs/heads/{branch}")) {
        eprintln!("land check: could not delete origin {branch}");
    }
    status
}

fn parse(arguments: &[String]) -> Option<Request> {
    let split = arguments.iter().position(|word| word == "--")?;
    let (flags, commands) = (&arguments[..split], &arguments[split + 1..]);
    let value = |name: &str| flags.chunks(2).find(|pair| pair[0] == name).and_then(|pair| pair.get(1)).cloned();
    if flags.len() != 8 || commands.is_empty() {
        return None;
    }
    Some(Request {
        workflow: value("--workflow")?,
        base: value("--base")?,
        head: value("--head")?,
        timeout: Duration::from_secs(value("--timeout")?.parse().ok()?),
        commands: commands.to_vec(),
    })
}

fn push(refspec: &str) -> bool {
    Command::new("git")
        .args(["push", "-q", "--no-verify", "origin", refspec])
        .env("SAFE_PUSH_ACTIVE", "1")
        .status()
        .is_ok_and(|status| status.success())
}

pub const INFRASTRUCTURE: u8 = 75;

#[derive(Debug, PartialEq)]
enum RefPush {
    Pushed,
    Failed,
    Infrastructure,
}

fn push_capturing(refspec: &str) -> (bool, String) {
    let output = Command::new("git").args(["push", "-q", "--no-verify", "origin", refspec]).env("SAFE_PUSH_ACTIVE", "1").output();
    let Ok(output) = output else { return (false, String::new()) };
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    eprint!("{stderr}");
    (output.status.success(), stderr)
}

fn transient_rejection(stderr: &str) -> bool {
    let rejected = stderr.contains("fatal error in commit_refs") || (stderr.contains("[remote rejected]") && stderr.contains("(failure)"));
    let server_error = stderr.lines().any(|line| {
        line.split(|c: char| !c.is_ascii_alphanumeric()).collect::<Vec<_>>().windows(2).any(|pair| {
            matches!(pair[0], "error" | "HTTP") && pair[1].len() == 3 && pair[1].starts_with('5') && pair[1].bytes().all(|b| b.is_ascii_digit())
        })
    });
    rejected || server_error
}

fn push_land_ref(refspec: &str, mut attempt: impl FnMut(&str) -> (bool, String), pause: impl FnOnce()) -> RefPush {
    let (pushed, stderr) = attempt(refspec);
    if pushed {
        return RefPush::Pushed;
    }
    if !transient_rejection(&stderr) {
        return RefPush::Failed;
    }
    eprintln!("safe-push: GitHub rejected the land-check ref push transiently (commit_refs); retrying once");
    pause();
    if attempt(refspec).0 { RefPush::Pushed } else { RefPush::Infrastructure }
}

fn gh(arguments: &[&str]) -> Result<String, String> {
    let output = process::run_within("gh", arguments, GH_LIMIT).ok_or_else(|| format!("gh {} timed out", arguments[..2].join(" ")))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(format!("gh {}: {}", arguments[..2].join(" "), String::from_utf8_lossy(&output.stderr).trim()))
}

fn check(request: &Request, branch: &str) -> u8 {
    let nonce = format!("{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos()));
    let title = format!("Land check {} {nonce}", request.head);
    let commands = json!(request.commands).to_string();
    let fields = [("base", request.base.as_str()), ("head", &request.head), ("nonce", &nonce), ("commands", &commands)].map(|(key, value)| format!("{key}={value}"));
    let mut dispatch = vec!["workflow", "run", request.workflow.as_str(), "--ref", branch];
    for field in &fields {
        dispatch.extend(["-f", field.as_str()]);
    }
    if let Err(why) = gh(&dispatch) {
        eprintln!("land check: {why}");
        return if dispatch_refused(&why) { infrastructure("GitHub refused the workflow dispatch") } else { 1 };
    }
    let list = ["run", "list", "--workflow", &request.workflow, "--branch", branch, "--event", "workflow_dispatch", "--limit", "20", "--json", "databaseId,displayTitle,url,status,conclusion"];
    let started = Instant::now();
    let mut announced = false;
    let mut delay = 5;
    loop {
        let found = gh(&list).ok().and_then(|runs| dispatched(&runs, &title));
        match &found {
            Some(run) if !announced => {
                println!("land check: {} command(s) on Actions: {}", request.commands.len(), text(run, "url"));
                announced = true;
            }
            Some(run) if queued(run) && started.elapsed() > Duration::from_secs(QUEUED_SECONDS) => {
                if let Some(id) = run.get("databaseId").and_then(Value::as_u64) {
                    let _ = gh(&["run", "cancel", &id.to_string()]);
                }
                return infrastructure(&format!("the run stayed queued past {QUEUED_SECONDS} s; cancelled"));
            }
            None if started.elapsed() > Duration::from_secs(DISPATCH_SECONDS) => {
                eprintln!("land check: no {} run titled {title} appeared within {DISPATCH_SECONDS} s", request.workflow);
                return infrastructure("the dispatched run never appeared");
            }
            _ => {}
        }
        let id = found.as_ref().and_then(|run| run.get("databaseId")).and_then(Value::as_u64).map(|id| id.to_string());
        match found.as_ref().map(verdict) {
            Some(Verdict::Passed) => return 0,
            Some(Verdict::Failed(conclusion)) => return report_failure(id.as_deref().unwrap_or_default(), &conclusion),
            _ => {}
        }
        if started.elapsed() >= request.timeout {
            if let Some(id) = &id {
                let _ = gh(&["run", "cancel", id]);
            }
            eprintln!("land check: the Actions run did not finish within {} s; cancelled", request.timeout.as_secs());
            return 124;
        }
        thread::sleep(Duration::from_secs(delay));
        delay = (delay * 3 / 2).min(30);
    }
}

fn infrastructure(why: &str) -> u8 {
    eprintln!("land check: {why}; infrastructure, not a check failure");
    INFRASTRUCTURE
}

fn dispatch_refused(why: &str) -> bool {
    why.ends_with("timed out") || why.contains("HTTP 403") || why.contains("HTTP 429") || transient_rejection(why)
}

fn queued(run: &Value) -> bool {
    matches!(text(run, "status"), "queued" | "pending" | "waiting" | "requested")
}

fn text<'a>(run: &'a Value, key: &str) -> &'a str {
    run.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn dispatched(runs: &str, title: &str) -> Option<Value> {
    let runs: Value = serde_json::from_str(runs).ok()?;
    runs.as_array()?.iter().find(|run| text(run, "displayTitle") == title).cloned()
}

fn verdict(run: &Value) -> Verdict {
    match (text(run, "status"), text(run, "conclusion")) {
        ("completed", "success") => Verdict::Passed,
        ("completed", conclusion) => Verdict::Failed(conclusion.to_owned()),
        _ => Verdict::Pending,
    }
}

fn report_failure(id: &str, conclusion: &str) -> u8 {
    let jobs = gh(&["run", "view", id, "--json", "jobs"]).ok().and_then(|jobs| serde_json::from_str::<Value>(&jobs).ok());
    let failed: Vec<String> = jobs
        .as_ref()
        .and_then(|jobs| jobs.get("jobs")?.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|job| text(job, "conclusion") != "success")
        .map(|job| format!("{} ({})", text(job, "name"), text(job, "conclusion")))
        .collect();
    if let Ok(log) = gh(&["run", "view", id, "--log-failed"]) {
        if let Some(path) = std::env::var_os("SAFE_PUSH_LAND_FAILED_LOG") {
            let _ = std::fs::write(path, &log);
        }
        let lines: Vec<&str> = log.lines().collect();
        for line in &lines[lines.len().saturating_sub(FAILED_LOG_LINES)..] {
            eprintln!("{line}");
        }
    }
    eprintln!("land check: the Actions run concluded {conclusion}; failed: {}", if failed.is_empty() { "unknown".to_owned() } else { failed.join(", ") });
    1
}

#[cfg(test)]
#[path = "land_actions_tests.rs"]
mod tests;
