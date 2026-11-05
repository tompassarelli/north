use std::env;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use super::date;
use super::skill_triggers::{self, HostProbe, RepositorySources, Skill};
use crate::foundation::clock::Timestamp;
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

pub(super) fn without_grounded(stale: &Value, owned: &[Skill]) -> Value {
    let kept = stale.as_array().into_iter().flatten().map(|entry| {
        let mut entry = entry.clone();
        let tracked = entry["id"].as_str().is_some_and(|id| owned.iter().any(|skill| skill.id == id && skill.grounded_day.is_some()));
        if let (true, Some(due)) = (tracked, entry["due"].as_object_mut()) {
            due.remove("grounded");
        }
        entry
    });
    Value::Array(kept.collect())
}

pub(super) fn due_count(stale: &Value) -> usize {
    stale.as_array().into_iter().flatten().filter_map(|entry| entry["due"].as_object()).map(|due| due.len()).sum()
}

pub(super) fn issue_body(today: &str, items: &str) -> String {
    format!(
        "Skill reviews due on {today}: `written` dates over 30 days from `agents stale`, and for grounding a skill's `cadence:` (default 30 days) elapsed, a stamped `<tool>-version:` that differs from `<tool> --version` on this host, or a cited `sources:` entry in docs/references.md or south agents/watch.toml changed since its `grounded` date. Grounding comes first: it rechecks facts against the real tools, versions and paths; `written` compresses prose and moves knowledge to its one owner without changing facts. Each review bumps its own frontmatter date, and its commit names which review it was.\n\n{items}"
    )
}

fn queue() -> Result<String, String> {
    let repo = std::env::var("SKILL_REVIEW_REPO").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "tompassarelli/north".to_owned());
    let stale = Command::new("agents").args(["stale", "--json"]).output().map_err(|error| format!("agents stale: {error}"))?;
    if !stale.status.success() {
        return Err("agents stale --json failed".to_owned());
    }
    let stale: Value = serde_json::from_slice(&stale.stdout).map_err(|error| format!("agents stale: {error}"))?;
    let today = date("-I");
    let home = PathBuf::from(env::var("HOME").unwrap_or_default());
    let north = env::var("NORTH_MAIN").ok().filter(|value| !value.is_empty()).map_or_else(|| home.join("code/north/main"), PathBuf::from);
    let watch_toml = env::var("WATCH_TOML").ok().filter(|value| !value.is_empty()).map_or_else(|| home.join("code/south/main/agents/watch.toml"), PathBuf::from);
    let skills = skill_triggers::load(&north.join("harness/skills"));
    let today_day = Timestamp::parse_loose(&today).map_or(0, Timestamp::unix_days);
    let triggered = skill_triggers::lines(&skills, today_day, &HostProbe, &RepositorySources { north: &north, watch_toml: &watch_toml });
    let stale = without_grounded(&stale, &skills);
    let count = due_count(&stale) + triggered.len();
    let listed = [items(&stale)].into_iter().chain(triggered).filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n");
    let body = (count > 0).then(|| issue_body(&today, &listed));
    settle(&mut Gh::of(&repo, &[]), TITLE, body.as_deref(), &format!("No skill review is due ({today})."))?;
    Ok(if count == 0 { "none due".to_owned() } else { format!("{count} due; {repo} \"{TITLE}\" is current") })
}
