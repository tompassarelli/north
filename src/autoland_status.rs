use crate::platform::{github, process};
use serde_json::Value;
use std::thread;
use std::time::Duration;

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub(crate) fn read_gh(arguments: &[&str]) -> Option<String> {
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

pub(crate) fn status(sha: &str, branch: &str, mut gh: impl FnMut(&[&str]) -> Option<String>) -> String {
    let endpoint = format!("repos/{{owner}}/{{repo}}/commits/{sha}/status");
    let current = gh(&[
        "api", &endpoint, "--jq",
        "([.statuses[] | select(.context == \"autoland\")][0] // {}) | [.state // \"\", .description // \"\", .target_url // \"\"] | @tsv",
    ]).unwrap_or_default();
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

fn wait_for_landing(
    sha: &str,
    destination: &str,
    timeout: u64,
    initial_delay: u64,
    mut read: impl FnMut() -> String,
    mut landed: impl FnMut(&str) -> Option<String>,
    mut pause: impl FnMut(u64),
) -> String {
    let mut waited = 0;
    let mut delay = initial_delay.max(1);
    let mut last = String::new();
    loop {
        let mut line = read();
        let mut fields = line.splitn(3, '\t');
        let state = fields.next().unwrap_or("");
        let description = fields.next().unwrap_or("");
        let url = fields.next().unwrap_or("");
        let landed_sha = if state == "success" {
            description
                .split_whitespace()
                .last()
                .filter(|value| {
                    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .unwrap_or(sha)
        } else {
            sha
        };
        if let Some(landed_sha) = landed(landed_sha) {
            return format!("success\tlanded {landed_sha}\t{url}");
        }
        if state == "failure" || state == "error" {
            return line;
        }
        if state == "success" {
            line = format!(
                "pending\tAutoland completed; waiting for origin/{destination} ancestry\t{url}"
            );
        }
        if line != last {
            last = line;
        }
        if waited >= timeout {
            return format!(
                "timeout\tafter {timeout} seconds\t{}",
                last.split('\t').nth(2).unwrap_or("")
            );
        }
        let sleep = delay.min(timeout - waited);
        pause(sleep);
        waited += sleep;
        delay = (delay * 3 / 2).min(60);
    }
}

struct WaitOptions {
    sha: String,
    branch: String,
    destination: String,
    timeout: u64,
    delay: u64,
}

fn parse_wait_options(arguments: &[String]) -> Result<WaitOptions, ()> {
    let mut sha = None;
    let mut branch = None;
    let mut destination = None;
    let mut timeout = 10_800;
    let mut delay = 10;
    let mut index = 0;
    while index + 1 < arguments.len() {
        match arguments[index].as_str() {
            "--sha" => sha = Some(arguments[index + 1].clone()),
            "--branch" => branch = Some(arguments[index + 1].clone()),
            "--destination" => destination = Some(arguments[index + 1].clone()),
            "--timeout-seconds" => timeout = arguments[index + 1].parse().unwrap_or(0),
            "--poll-seconds" => delay = arguments[index + 1].parse().unwrap_or(0),
            _ => return Err(()),
        }
        index += 2;
    }
    match (sha, branch, destination) {
        (Some(sha), Some(branch), Some(destination)) => Ok(WaitOptions {
            sha,
            branch,
            destination,
            timeout,
            delay,
        }),
        _ => Err(()),
    }
}

pub fn wait(arguments: &[String]) -> u8 {
    let options = match parse_wait_options(arguments) {
        Ok(options) => options,
        Err(()) => {
            eprintln!(
                "Usage: north autoland-wait --sha SHA --branch BRANCH --destination BRANCH [--timeout-seconds N] [--poll-seconds N]"
            );
            return 2;
        }
    };
    let result = wait_for_landing(
        &options.sha,
        &options.destination,
        options.timeout,
        options.delay,
        || status(&options.sha, &options.branch, read_gh),
        |candidate| {
            let fetch = [
                "fetch",
                "-q",
                "origin",
                &format!(
                    "+refs/heads/{0}:refs/remotes/origin/{0}",
                    options.destination
                ),
            ];
            let _ = process::run_within("git", &fetch, Duration::from_secs(30));
            landed_commit(
                candidate,
                &format!("refs/remotes/origin/{}", options.destination),
            )
        },
        |seconds| thread::sleep(Duration::from_secs(seconds)),
    );
    println!("{result}");
    if result.starts_with("timeout\t") {
        2
    } else {
        0
    }
}

pub(crate) fn git_output(arguments: &[&str]) -> Option<String> {
    process::run_within("git", arguments, Duration::from_secs(30))
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn patch_ids(arguments: &[&str]) -> Option<String> {
    let patches = git_output(arguments)?;
    process::run_with_input_within(
        "git",
        &["patch-id", "--stable"],
        Some(patches.as_bytes()),
        Duration::from_secs(30),
    )
    .filter(|output| output.status.success())
    .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn landed_commit(candidate: &str, destination: &str) -> Option<String> {
    if process::run_within(
        "git",
        &["merge-base", "--is-ancestor", candidate, destination],
        Duration::from_secs(30),
    )
    .is_some_and(|output| output.status.success())
    {
        return git_output(&["rev-parse", "--verify", &format!("{candidate}^{{commit}}")])
            .map(|sha| sha.trim().to_owned());
    }
    let base = git_output(&["merge-base", candidate, destination])?;
    let original = patch_ids(&[
        "show",
        "--no-ext-diff",
        "--no-color",
        "--binary",
        "--format=commit %H",
        candidate,
    ])?;
    let patch = original.split_whitespace().next()?;
    let range = format!("{}..{destination}", base.trim());
    let landed = patch_ids(&[
        "log",
        "--no-ext-diff",
        "--no-color",
        "--binary",
        "--format=commit %H",
        "-p",
        "--no-merges",
        &range,
    ])?;
    landed.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == patch)
            .then(|| fields.next().map(str::to_owned))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::{status, wait_for_landing};
    use serde_json::{Value, json};
    use std::cell::Cell;

    #[test]
    fn workflow_refusal_matches_latest_exact_dispatched_lane() {
        let branch = "claude/land-soak-split-c38556cf0b53";
        let title = format!("Autoland test 0 {branch} queued-sha");
        let mut runs = json!([
            {"databaseId":30,"displayTitle":title,"event":"workflow_dispatch","status":"in_progress","conclusion":null,"url":"https://example.test/current"},
            {"databaseId":20,"displayTitle":title,"event":"workflow_dispatch","status":"completed","conclusion":"failure","url":"https://example.test/old"},
            {"databaseId":100,"displayTitle":"Autoland test 0 another-lane queued-sha","event":"workflow_dispatch","status":"completed","conclusion":"failure","url":"https://example.test/other"},
            {"databaseId":101,"displayTitle":title,"event":"push","status":"completed","conclusion":"failure","url":"https://example.test/push"},
            {"databaseId":102,"displayTitle":"Autoland plan","event":"workflow_dispatch","status":"completed","conclusion":"failure","url":"https://example.test/plan"}
        ]);
        let read = |runs: &Value| {
            status("queued-sha", branch, |arguments| {
                Some(if arguments[0] == "api" {
                    "\t\t\n".into()
                } else {
                    runs.to_string()
                })
            })
        };
        assert_eq!(
            read(&runs),
            "pending\tAutoland workflow in_progress\thttps://example.test/current"
        );
        runs[0]["status"] = json!("completed");
        runs[0]["conclusion"] = json!("failure");
        assert_eq!(
            read(&runs),
            "failure\tAutoland workflow failure\thttps://example.test/current"
        );
        runs[0]["conclusion"] = json!("success");
        assert_eq!(
            read(&runs),
            "success\tAutoland workflow success\thttps://example.test/current"
        );
    }

    #[test]
    fn origin_ancestry_ends_wait_when_autoland_status_is_missing() {
        let reads = Cell::new(0);
        let sleeps = Cell::new(0);
        let sha = "a".repeat(40);
        let line = wait_for_landing(
            &sha,
            "main",
            3,
            1,
            || "\t\tnot queued yet".into(),
            |_| {
                let count = reads.get() + 1;
                reads.set(count);
                (count > 1).then(|| sha.clone())
            },
            |seconds| sleeps.set(sleeps.get() + seconds),
        );
        assert!(line.starts_with("success\t"));
        assert_eq!(reads.get(), 2);
        assert_eq!(sleeps.get(), 1);
    }

    #[test]
    fn autoland_timeout_keeps_the_run_url() {
        let line = wait_for_landing(
            "a",
            "main",
            1,
            1,
            || "pending\tAutoland workflow queued\thttps://example.test/pending".into(),
            |_| None,
            |_| {},
        );
        assert_eq!(
            line,
            "timeout\tafter 1 seconds\thttps://example.test/pending"
        );
    }
}
