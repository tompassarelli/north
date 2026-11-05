use std::process::Command;

use serde_json::Value;

use super::date;
use crate::platform::rolling_issue::{Gh, settle};

const TITLE: &str = "Skill review queue";

pub fn run(_arguments: &[String]) -> u8 {
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
    let count = due_count(&stale);
    let today = date("-I");
    let body = (count > 0).then(|| issue_body(&today, &items(&stale)));
    settle(&mut Gh::of(&repo, &[]), TITLE, body.as_deref(), &format!("No skill review is due ({today})."))?;
    Ok(if count == 0 { "none due".to_owned() } else { format!("{count} due; {repo} \"{TITLE}\" is current") })
}
