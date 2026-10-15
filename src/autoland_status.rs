use serde_json::Value;
use std::process::Command;

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn status(sha: &str, branch: &str, mut gh: impl FnMut(&[&str]) -> Option<String>) -> String {
    let endpoint = format!("repos/{{owner}}/{{repo}}/commits/{sha}/status");
    let current = gh(&[
        "api", &endpoint, "--jq",
        "([.statuses[] | select(.context == \"autoland\")][0] // {}) | [.state // \"\", .description // \"\", .target_url // \"\"] | @tsv",
    ]).unwrap_or_default();
    if matches!(current.split('\t').next().unwrap_or("").trim(), "success" | "failure" | "error") {
        return current.trim_end().to_owned();
    }
    let payload = gh(&[
        "run", "list", "--workflow", "autoland.yml", "--event", "workflow_dispatch",
        "--limit", "100", "--json", "databaseId,displayTitle,event,status,conclusion,url",
    ]);
    let runs = payload.as_deref().and_then(|payload| serde_json::from_str::<Value>(payload).ok());
    let expected = format!("Autoland {branch}");
    let latest = runs.as_ref().and_then(Value::as_array).and_then(|runs| {
        runs.iter().filter(|run| {
            text(run, "displayTitle") == Some(expected.as_str())
                && text(run, "event") == Some("workflow_dispatch")
        }).max_by_key(|run| run.get("databaseId").and_then(Value::as_u64).unwrap_or(0))
    });
    if let Some(run) = latest
        && text(run, "status") == Some("completed")
        && let Some(conclusion @ ("failure" | "cancelled" | "timed_out" | "action_required" | "startup_failure")) = text(run, "conclusion")
        && let Some(url) = text(run, "url")
    {
        return format!("failure\tAutoland workflow {conclusion}\t{url}");
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
    println!("{}", status(sha, branch, |arguments| {
        Command::new("gh").args(arguments).output().ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
    }));
    0
}

#[cfg(test)]
mod tests {
    use super::status;
    use serde_json::{Value, json};

    #[test]
    fn workflow_refusal_matches_latest_exact_dispatched_lane() {
        let branch = "claude/land-soak-split-c38556cf0b53";
        let title = format!("Autoland {branch}");
        let mut runs = json!([
            {"databaseId":30,"displayTitle":title,"event":"workflow_dispatch","status":"in_progress","conclusion":null,"url":"https://example.test/current"},
            {"databaseId":20,"displayTitle":title,"event":"workflow_dispatch","status":"completed","conclusion":"failure","url":"https://example.test/old"},
            {"databaseId":100,"displayTitle":"Autoland another-lane","event":"workflow_dispatch","status":"completed","conclusion":"failure","url":"https://example.test/other"},
            {"databaseId":101,"displayTitle":title,"event":"push","status":"completed","conclusion":"failure","url":"https://example.test/push"}
        ]);
        let read = |runs: &Value| status("queued-sha", branch, |arguments| {
            Some(if arguments[0] == "api" { "\t\t\n".into() } else { runs.to_string() })
        });
        assert_eq!(read(&runs), "");
        runs[0]["status"] = json!("completed");
        runs[0]["conclusion"] = json!("failure");
        assert_eq!(read(&runs), "failure\tAutoland workflow failure\thttps://example.test/current");
        runs[0]["conclusion"] = json!("success");
        assert_eq!(read(&runs), "");
    }
}
