use std::fs;
use std::path::Path;
use std::process::Command;

use north_base::error::NorthError;
use north_base::foundation::clock::Timestamp;
use serde::Deserialize;

use super::refusal;
use super::task::sha256;
use crate::ab::{AbRow, Usage, parse};
use crate::rows::Arm;

pub const REPOS: [&str; 2] = ["tompassarelli/wisp", "tompassarelli/makgora"];
pub const SEED: u64 = 877;
pub const T3_MODEL: &str = "claude-opus-5-5";
pub const T3_EFFORT: &str = "default";
pub const NORTH_PENDING: &str = "route-pick";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Box {
    pub id: String,
    pub text: String,
    pub rank: u8,
    pub number: u64,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
    body: String,
    labels: Vec<Label>,
}

fn slug(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(8)
        .map(str::to_ascii_lowercase)
        .collect();
    words.join("-")
}

fn rank(labels: &[Label]) -> u8 {
    ["priority:now", "priority:next", "priority:later"]
        .iter()
        .position(|name| labels.iter().any(|label| label.name == *name))
        .map_or(3, |index| index as u8)
}

/// Unticked boxes of an issue's Done when whose text names a command in backticks and whose labels do not hand the close to Tom or a native client.
fn runnable_boxes(repo: &str, issue: &Issue) -> Vec<Box> {
    if issue
        .labels
        .iter()
        .any(|label| label.name.starts_with("needs:") || label.name.starts_with("evidence:"))
    {
        return Vec::new();
    }
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let mut in_done = false;
    let mut found = Vec::new();
    for line in issue.body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_done = heading.trim().eq_ignore_ascii_case("done when");
        } else if in_done
            && let Some(text) = line.trim_start().strip_prefix("- [ ] ")
            && text.matches('`').count() >= 2
            && !text.starts_with("(decision)")
        {
            found.push(Box {
                id: format!("{name}#{}:{}", issue.number, slug(text)),
                text: text.to_owned(),
                rank: rank(&issue.labels),
                number: issue.number,
            });
        }
    }
    found
}

fn open_boxes(repo: &str) -> Result<Vec<Box>, NorthError> {
    let out = Command::new("gh")
        .args([
            "issue",
            "list",
            "-R",
            repo,
            "--state",
            "open",
            "--limit",
            "500",
            "--json",
            "number,body,labels",
        ])
        .output()
        .map_err(NorthError::Io)?;
    if !out.status.success() {
        return Err(refusal(format!(
            "gh issue list -R {repo} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let issues: Vec<Issue> = serde_json::from_slice(&out.stdout).map_err(NorthError::Json)?;
    Ok(issues
        .iter()
        .flat_map(|issue| runnable_boxes(repo, issue))
        .collect())
}

/// The coin: the parity of the first byte of sha256 over the seed and the box, so a box's arm is fixed by the recorded seed alone.
pub fn flip(seed: u64, box_id: &str) -> Arm {
    if sha256(&format!("{seed}:{box_id}")).as_bytes()[0].is_multiple_of(2) {
        Arm::North
    } else {
        Arm::T3Code
    }
}

pub fn load(file: &Path) -> Result<Vec<AbRow>, NorthError> {
    match fs::read_to_string(file) {
        Ok(text) => parse(&text).map_err(refusal),
        Err(_) => Ok(Vec::new()),
    }
}

fn save(file: &Path, rows: &[AbRow]) -> Result<(), NorthError> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(NorthError::Io)?;
    }
    let lines: Vec<String> = rows
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<_, _>>()
        .map_err(NorthError::Json)?;
    fs::write(file, lines.join("\n") + "\n").map_err(NorthError::Io)
}

pub fn assign(
    file: &Path,
    repos: &[String],
    seed: u64,
    count: usize,
) -> Result<String, NorthError> {
    let mut rows = load(file)?;
    let mut open = Vec::new();
    for repo in repos {
        open.extend(open_boxes(repo)?);
    }
    open.retain(|candidate| !rows.iter().any(|row| row.task == candidate.id));
    open.sort_by(|a, b| (a.rank, a.number, &a.id).cmp(&(b.rank, b.number, &b.id)));
    let now = Timestamp::now().unix_seconds();
    let mut text = String::new();
    for picked in open.into_iter().take(count) {
        let arm = flip(seed, &picked.id);
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            picked.id,
            arm.label(),
            picked.text
        ));
        rows.push(AbRow {
            task: picked.id,
            arm,
            assigned_at: now,
            model: if arm == Arm::T3Code {
                T3_MODEL
            } else {
                NORTH_PENDING
            }
            .to_owned(),
            effort: if arm == Arm::T3Code {
                T3_EFFORT
            } else {
                NORTH_PENDING
            }
            .to_owned(),
            landed_sha: None,
            landed_at: None,
            usage: Usage::default(),
        });
    }
    save(file, &rows)?;
    Ok(text)
}

pub struct Landing<'a> {
    pub task: &'a str,
    pub sha: &'a str,
    pub at: i64,
    pub usage: Usage,
    pub model: Option<String>,
    pub effort: Option<String>,
}

pub fn record(file: &Path, landing: &Landing) -> Result<String, NorthError> {
    if landing.sha.len() < 7 || !landing.sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(refusal(format!("{} is not a commit sha", landing.sha)));
    }
    let mut rows = load(file)?;
    let row = rows
        .iter_mut()
        .find(|row| row.task == landing.task)
        .ok_or_else(|| {
            refusal(format!(
                "{} was never assigned in {}",
                landing.task,
                file.display()
            ))
        })?;
    row.landed_sha = Some(landing.sha.to_owned());
    row.landed_at = Some(landing.at);
    row.usage.tokens_in += landing.usage.tokens_in;
    row.usage.tokens_out += landing.usage.tokens_out;
    row.usage.cost_usd += landing.usage.cost_usd;
    if let Some(model) = &landing.model {
        row.model.clone_from(model);
    }
    if let Some(effort) = &landing.effort {
        row.effort.clone_from(effort);
    }
    let text = format!(
        "{}\t{}\tlanded {}\n",
        row.task,
        row.arm.label(),
        landing.sha
    );
    save(file, &rows)?;
    Ok(text)
}
