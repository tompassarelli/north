use crate::platform::{github, process};
use serde_json::Value;
use std::time::Duration;

mod evidence;
mod hold;
pub use evidence::{git_output, landed_commit};
pub use hold::{deletion_hold, hook_deadline, origin_branches_hold};

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub fn read_gh(arguments: &[&str]) -> Option<String> {
    process::run_within("gh", arguments, Duration::from_secs(30))
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn exact_run(payload: Option<String>, branch: &str, sha: &str) -> Option<Value> {
    let runs = payload.and_then(|payload| serde_json::from_str::<Value>(&payload).ok());
    let expected_suffix = format!(" {branch} {sha}");
    runs.as_ref().and_then(Value::as_array).and_then(|runs| {
        runs.iter()
            .filter(|run| {
                text(run, "displayTitle")
                    .and_then(|title| title.strip_prefix("Autoland test "))
                    .and_then(|title| title.strip_suffix(&expected_suffix))
                    .is_some_and(|slot| {
                        !slot.is_empty() && slot.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    && text(run, "event") == Some("workflow_dispatch")
            })
            .max_by_key(|run| run.get("databaseId").and_then(Value::as_u64).unwrap_or(0))
            .cloned()
    })
}

fn workflow_result(run: &Value) -> Option<String> {
    let url = text(run, "url")?;
    if matches!(text(run, "status"), Some("in_progress" | "queued")) {
        return Some(format!(
            "pending\tAutoland workflow {}\t{url}",
            text(run, "status").unwrap_or("queued")
        ));
    }
    let conclusion = text(run, "conclusion")?;
    (text(run, "status") == Some("completed")
        && matches!(
            conclusion,
            "success"
                | "failure"
                | "cancelled"
                | "timed_out"
                | "action_required"
                | "startup_failure"
        ))
    .then(|| {
        format!(
            "{}\tAutoland workflow {conclusion}\t{url}",
            if conclusion == "success" {
                "success"
            } else {
                "failure"
            }
        )
    })
}

fn commit_status(sha: &str, gh: &mut impl FnMut(&[&str]) -> Option<String>) -> Option<String> {
    let endpoint = format!("repos/{{owner}}/{{repo}}/commits/{sha}/status");
    gh(&[
        "api", &endpoint, "--jq",
        "([.statuses[] | select(.context == \"autoland\")][0] // {}) | [.state // \"\", .description // \"\", .target_url // \"\"] | @tsv",
    ])
}

pub fn status(sha: &str, branch: &str, mut gh: impl FnMut(&[&str]) -> Option<String>) -> String {
    let current = commit_status(sha, &mut gh).unwrap_or_default();
    if matches!(
        current.split('\t').next().unwrap_or("").trim(),
        "success" | "failure" | "error"
    ) {
        return current.trim_end().to_owned();
    }
    let payload = gh(&[
        "run",
        "list",
        "--workflow",
        "autoland.yml",
        "--event",
        "workflow_dispatch",
        "--limit",
        "100",
        "--json",
        "databaseId,displayTitle,event,status,conclusion,url",
    ]);
    if let Some(run) = exact_run(payload, branch, sha)
        && let Some(result) = workflow_result(&run)
    {
        return result;
    }
    current.trim_end().to_owned()
}

pub fn run(arguments: &[String]) -> u8 {
    let [sha_flag, sha, branch_flag, branch] = arguments else {
        eprintln!("Usage: north autoland-status --sha SHA --branch BRANCH");
        return 2;
    };
    if sha_flag != "--sha" || branch_flag != "--branch" || sha.is_empty() || branch.is_empty() {
        eprintln!("Usage: north autoland-status --sha SHA --branch BRANCH");
        return 2;
    }
    println!(
        "{}",
        status(sha, branch, |arguments| {
            github::run(arguments)
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
        })
    );
    0
}
