use std::process::Command;

use serde_json::Value;

use super::{date, text};

const TITLE: &str = "Skill review queue";

pub(crate) fn run(_arguments: &[String]) -> u8 {
    match queue() {
        Ok(line) => {
            println!("skill-review-queue: {line}");
            0
        }
        Err(error) => {
            eprintln!("skill-review-queue: {error}");
            1
        }
    }
}

fn gh(args: &[&str]) -> Result<String, String> {
    let output = crate::platform::github::run(args).map_err(|error| format!("cannot run gh: {error}"))?;
    if !output.status.success() {
        return Err(format!("gh {} failed: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(text(&output))
}

pub(super) fn items(stale: &Value) -> String {
    let entries = stale.as_array().map(Vec::as_slice).unwrap_or_default();
    ["grounded", "written"]
        .iter()
        .flat_map(|field| {
            entries.iter().filter_map(move |entry| {
                let due = &entry["due"][*field];
                if due.is_null() || due == &Value::Bool(false) {
                    return None;
                }
                let show = |value: &Value| value.as_str().map_or_else(|| value.to_string(), str::to_owned);
                Some(format!("- [ ] `{}` {field} (last {}, {} days)", show(&entry["id"]), show(&due["date"]), show(&due["ageDays"])))
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn due_count(stale: &Value) -> usize {
    stale.as_array().into_iter().flatten().filter_map(|entry| entry["due"].as_object()).map(|due| due.len()).sum()
}

pub(super) fn issue_body(today: &str, items: &str) -> String {
    format!(
        "Skill reviews older than 30 days, from `agents stale` on {today}. Grounding comes first: it rechecks facts against the real tools, versions and paths; `written` compresses prose and moves knowledge to its one owner without changing facts. Each review bumps its own frontmatter date, and its commit names which review it was.\n\n{items}"
    )
}

fn queue() -> Result<String, String> {
    let repo = std::env::var("SKILL_REVIEW_REPO").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "tompassarelli/north".to_owned());
    let stale = Command::new("agents").args(["stale", "--json"]).output().map_err(|error| format!("agents stale: {error}"))?;
    if !stale.status.success() {
        return Err("agents stale --json failed".to_owned());
    }
    let stale: Value = serde_json::from_slice(&stale.stdout).map_err(|error| format!("agents stale: {error}"))?;
    let search = format!("\"{TITLE}\" in:title");
    let pick = format!("map(select(.title == \"{TITLE}\")) | .[0].number // empty");
    let listed = gh(&["issue", "list", "-R", &repo, "--state", "open", "--search", &search, "--json", "number,title", "--jq", &pick])?;
    let existing = listed.trim_end_matches('\n');
    let count = due_count(&stale);
    let today = date("-I");
    if count == 0 {
        if existing.is_empty() {
            return Ok("none due".to_owned());
        }
        gh(&["issue", "close", "-R", &repo, existing, "--comment", &format!("No skill review is due ({today}).")])?;
        return Ok(format!("none due; closed {repo}#{existing}"));
    }
    let body = issue_body(&today, &items(&stale));
    if existing.is_empty() {
        let url = gh(&["issue", "create", "-R", &repo, "--title", TITLE, "--body", &body])?;
        Ok(format!("{count} due; opened {}", url.trim_end_matches('\n')))
    } else {
        gh(&["issue", "edit", "-R", &repo, existing, "--body", &body])?;
        Ok(format!("{count} due; updated {repo}#{existing}"))
    }
}
