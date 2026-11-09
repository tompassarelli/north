//! The unrunnable-box gate (north#872): an issue created or edited with a new unticked box that names no backticked `Done when:` command
//! can only be ticked by its own worker. While harness/gates.toml has it in shadow, each such box is logged to
//! `$XDG_STATE_HOME/north/gates/unrunnable-box.jsonl`; once it enforces, the filing is refused.
use super::gh_write::{self, GhWrite};
use super::issue_labels::gh;
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const GATE: &str = "unrunnable-box";
const OWNER: &str = "tompassarelli/";
const IDENTITY_CHARS: usize = 60;

static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t>]*[-*][ \t]+\[ \][ \t]+(.*)$").unwrap());
static RUNNABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)Done when:[^`]*`[^`]+`").unwrap());
static EXEMPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bNeeds:|\bnot before\b").unwrap());

pub(super) trait World {
    fn repo(&mut self, named: Option<&str>) -> Option<String>;
    fn current(&mut self, repo: &str, number: &str) -> Option<String>;
    fn record(&mut self, row: serde_json::Value);
}

struct Live {
    cwd: Option<String>,
    state: PathBuf,
}

impl World for Live {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        match named {
            Some(repo) => Some(repo.to_owned()),
            None => gh(self.cwd.as_deref(), &["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"]).map(|out| out.trim().to_owned()),
        }
    }

    fn current(&mut self, repo: &str, number: &str) -> Option<String> {
        gh(None, &["issue", "view", number, "-R", repo, "--json", "body", "-q", ".body"])
    }

    fn record(&mut self, row: serde_json::Value) {
        let directory = self.state.join("north/gates");
        let _ = std::fs::create_dir_all(&directory);
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(format!("{GATE}.jsonl"))) {
            let _ = writeln!(file, "{row}");
        }
    }
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let Some(state) = var("XDG_STATE_HOME").or_else(|| var("HOME").map(|home| home.join(".local/state"))) else {
        return Verdict::Allow;
    };
    let enforcing = north_base::platform::harness::document("harness/gates.toml")["gate"][GATE]["mode"].as_str() == Some("enforce");
    let refused = judge(input, &mut Live { cwd: input.cwd().map(str::to_owned), state }, !enforcing);
    if enforcing && !refused.is_empty() {
        return Verdict::Deny(refusal(&refused));
    }
    Verdict::Allow
}

fn refusal(boxes: &[String]) -> String {
    let listed: String = boxes.iter().map(|text| format!("\n  - [ ] {}", excerpt(text))).collect();
    format!(
        "unrunnable-box: a new unticked box must name a command that `north task check --done-when` can run, so a checker and not its own worker ticks it (north#872). These boxes name none:{listed}\nEnd each box with \"Done when:\" and the command in backticks, whose exit 0 shows the box done, for example: - [ ] Fix the parser. Done when: `cargo test -p north-base parser` passes on origin/main. A box that waits on Tom says \"Needs: Tom\"; a dated reading says \"not before YYYY-MM-DD\"."
    )
}

fn excerpt(text: &str) -> String {
    let head: String = text.chars().take(100).collect();
    if head.len() < text.len() { format!("{head}...") } else { head }
}

/// The new unrunnable boxes of every issue the command files or edits, logged to the gate's jsonl when `log` is set.
pub(super) fn judge(input: &HookInput, world: &mut impl World, log: bool) -> Vec<String> {
    let mut refused = Vec::new();
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return refused;
    }
    let Some(text) = input.command() else {
        return refused;
    };
    let session = std::env::var(crate::fleet::runtime::SESSION_ENV).unwrap_or_default();
    for write in gh_write::writes(&text.lossy(), input.cwd()) {
        let edited = write.edits.as_ref().filter(|(group, _)| group == "issue").map(|(_, number)| number.clone());
        if !write.creates_issue && edited.is_none() {
            continue;
        }
        let boxes = unrunnable(&body(&write));
        if boxes.is_empty() {
            continue;
        }
        let Some(repo) = world.repo(write.repo.as_deref()).filter(|repo| repo.to_lowercase().starts_with(OWNER)) else {
            continue;
        };
        let held: BTreeSet<String> = match &edited {
            Some(number) => match world.current(&repo, number) {
                Some(current) => current.lines().filter_map(|line| UNTICKED.captures(line)).map(|found| identity(&found[1])).collect(),
                None => continue,
            },
            None => BTreeSet::new(),
        };
        let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
        for (_, text) in boxes.into_iter().filter(|(_, text)| !held.contains(&identity(text))) {
            if log {
                world.record(serde_json::json!({"gate": GATE, "at": at, "would_refuse": true, "verb": if edited.is_some() { "edit" } else { "create" }, "repo": repo, "issue": edited, "session": session, "box": text}));
            }
            refused.push(text);
        }
    }
    refused
}

/// What names a box across edits of its issue: its opening words, since an edit that adds a `Measured` line or an evidence note to a held box rewrites the rest.
fn identity(text: &str) -> String {
    text.trim().chars().take(IDENTITY_CHARS).collect()
}

fn body(write: &GhWrite) -> String {
    write.bodies.iter().filter(|text| write.title.as_deref() != Some(text.as_str())).map(String::as_str).collect::<Vec<_>>().join("\n")
}

/// Each unticked box of `body` naming no backticked `Done when:` command and not saying it waits on `Needs:` or a date, as its line and its text.
pub(super) fn unrunnable(body: &str) -> Vec<(String, String)> {
    body.lines()
        .filter_map(|line| UNTICKED.captures(line).map(|found| (line.to_owned(), found[1].trim().to_owned())))
        .filter(|(_, text)| !RUNNABLE.is_match(text) && !EXEMPT.is_match(text))
        .collect()
}

#[cfg(test)]
#[path = "unrunnable_box_tests.rs"]
mod tests;
