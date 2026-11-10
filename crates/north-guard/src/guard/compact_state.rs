//! After a compaction, a fleet session starts again with what the summary may have dropped: its brief, its lane, the
//! open boxes of its Item and its registered waits. Claude Code runs SessionStart with source `compact` at that point.

use crate::fleet::model::SessionRecord;
use crate::fleet::records::read_record;
use crate::fleet::runtime::SESSION_ENV;
use crate::fleet::waits::Waits;
use crate::hook_input::{HookInput, Verdict};
use crate::platform::{process, repo_names};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const SOURCE: &str = "compact";
const COMMAND_LIMIT: Duration = Duration::from_secs(5);
const BRIEF_CHARS: usize = 8000;
const BOX_CHARS: usize = 300;
const BOXES_SHOWN: usize = 12;
const COMMITS_SHOWN: usize = 10;
const TRANSCRIPT_LINES: usize = 400;

pub fn evaluate(input: &HookInput) -> Verdict {
    if input.data["source"] != SOURCE {
        return Verdict::Allow;
    }
    let name = std::env::var(SESSION_ENV).unwrap_or_default();
    let Some(root) = crate::fleet::registry::root_here().ok().filter(|_| !name.is_empty()) else { return Verdict::Allow };
    let Ok(record) = read_record(&root.join(&name)) else { return Verdict::Allow };
    let transcript = input.data["transcript_path"].as_str().map(PathBuf::from);
    let cwd = input.data["cwd"].as_str().map_or_else(|| record.cwd.clone(), PathBuf::from);
    let state = State {
        brief: transcript.as_deref().and_then(first_prompt),
        lane: lane(&cwd),
        open_boxes: record.task_ref.as_deref().map(open_boxes).unwrap_or_default(),
        waits: Waits::in_root(&root).of(&name).iter().map(|wait| wait.label.as_ref().map_or_else(|| wait.run.to_string(), |label| format!("{} ({label})", wait.run))).collect(),
    };
    Verdict::Advise(state.render(&record))
}

pub(super) struct State {
    pub brief: Option<String>,
    pub lane: Lane,
    pub open_boxes: Vec<String>,
    pub waits: Vec<String>,
}

#[derive(Default)]
pub(super) struct Lane {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub unlanded: Vec<String>,
    pub dirty_files: usize,
}

impl State {
    pub(super) fn render(&self, record: &SessionRecord) -> String {
        let mut out = format!("You were compacted. The summary above may have dropped your task state, so it is restated here from the fleet record of {}:\n", record.name);
        match &self.brief {
            Some(brief) => out.push_str(&format!("\nYour brief, as launched:\n{}\n", clip(brief, BRIEF_CHARS))),
            None => out.push_str("\nYour brief could not be read from the transcript; reread it from the first message of this conversation.\n"),
        }
        out.push_str(&format!("\nYour lane: {}", self.lane.path.display()));
        if let Some(branch) = &self.lane.branch {
            out.push_str(&format!(", branch {branch}"));
        }
        out.push_str(&format!(", {} uncommitted file(s).\n", self.lane.dirty_files));
        if self.lane.unlanded.is_empty() {
            out.push_str("No commits of this branch are unlanded.\n");
        } else {
            out.push_str("Commits not yet on origin/main:\n");
            self.lane.unlanded.iter().for_each(|commit| out.push_str(&format!("- {commit}\n")));
        }
        if let Some(item) = &record.task_ref {
            out.push_str(&format!("\nUnticked boxes of {item}:\n"));
            if self.open_boxes.is_empty() {
                out.push_str("- none read; check the issue before you report\n");
            }
            self.open_boxes.iter().take(BOXES_SHOWN).for_each(|line| out.push_str(&format!("{}\n", clip(line, BOX_CHARS))));
        }
        out.push_str("\nRegistered background waits:\n");
        if self.waits.is_empty() {
            out.push_str("- none\n");
        }
        self.waits.iter().for_each(|wait| out.push_str(&format!("- {wait}\n")));
        out.push_str("\nContinue from here; do not report until your boxes are ticked or you are blocked.");
        out
    }
}

fn clip(text: &str, limit: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

pub(super) fn first_prompt(transcript: &Path) -> Option<String> {
    let lines = BufReader::new(std::fs::File::open(transcript).ok()?).lines().take(TRANSCRIPT_LINES);
    lines.map_while(Result::ok).find_map(|line| {
        let record: serde_json::Value = serde_json::from_str(&line).ok()?;
        if record["type"] != "user" || record["isMeta"] == true || record["isSidechain"] == true {
            return None;
        }
        let content = &record["message"]["content"];
        let text = content.as_str().map(str::to_owned).or_else(|| {
            let blocks: Vec<&str> = content.as_array()?.iter().filter_map(|block| block["text"].as_str()).collect();
            (!blocks.is_empty()).then(|| blocks.join("\n"))
        })?;
        Some(text).filter(|text| !text.trim().is_empty() && !text.starts_with('<'))
    })
}

fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args(args);
    let output = process::run_command_within(command, None, COMMAND_LIMIT)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn lane(cwd: &Path) -> Lane {
    let limit = format!("-n{COMMITS_SHOWN}");
    Lane {
        path: cwd.to_path_buf(),
        branch: git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|branch| !branch.is_empty()),
        unlanded: git(cwd, &["log", "--oneline", &limit, "origin/main..HEAD"]).map(|log| log.lines().map(|line| clip(line, BOX_CHARS)).collect()).unwrap_or_default(),
        dirty_files: git(cwd, &["status", "--porcelain"]).map_or(0, |status| status.lines().count()),
    }
}

fn open_boxes(task_ref: &str) -> Vec<String> {
    let Some((repo, number)) = task_ref.rsplit_once('#') else { return Vec::new() };
    let repo = repo_names::checkout_name(repo_names::repo_alias(repo.strip_prefix("tompassarelli/").unwrap_or(repo)));
    let mut command = Command::new("gh");
    command.args(["issue", "view", number, "--repo", &format!("tompassarelli/{repo}"), "--json", "body", "--jq", ".body"]);
    let Some(output) = process::run_command_within(command, None, COMMAND_LIMIT).filter(|output| output.status.success()) else { return Vec::new() };
    unticked(&String::from_utf8_lossy(&output.stdout))
}

pub(super) fn unticked(body: &str) -> Vec<String> {
    body.lines().filter(|line| line.trim_start().starts_with("- [ ]")).map(|line| line.trim().to_owned()).collect()
}

#[cfg(test)]
#[path = "compact_state_tests.rs"]
mod tests;
