//! The unrunnable-box gate in shadow (north#872): an issue created or edited with a new unticked box that names no
//! backticked `Done when:` command can only be ticked by its own worker, so each such box is logged to
//! `$XDG_STATE_HOME/north/gates/unrunnable-box.jsonl`. It refuses nothing until harness/gates.toml enforces it.
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
    shadow(input, &mut Live { cwd: input.cwd().map(str::to_owned), state });
    Verdict::Allow
}

pub(super) fn shadow(input: &HookInput, world: &mut impl World) {
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return;
    }
    let Some(text) = input.command() else {
        return;
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
                Some(current) => current.lines().map(|line| line.trim().to_owned()).collect(),
                None => continue,
            },
            None => BTreeSet::new(),
        };
        let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
        for (_, text) in boxes.into_iter().filter(|(line, _)| !held.contains(line.trim())) {
            world.record(serde_json::json!({"gate": GATE, "at": at, "would_refuse": true, "verb": if edited.is_some() { "edit" } else { "create" }, "repo": repo, "issue": edited, "session": session, "box": text}));
        }
    }
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
