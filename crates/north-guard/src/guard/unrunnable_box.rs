//! The unrunnable-box gate (north#872): an issue created or edited with a new unticked box that names no backticked `Done when:` command
//! can only be ticked by its own worker. `north gh-session` judges the body each fleet `gh issue create` or `gh issue edit` sends when gh
//! runs (`gh_screen`), so the gate holds for every model provider and for a body file the command wrote, which a shell hook reading the
//! command text missed (north#1041). While harness/gates.toml has it in shadow, each such box is logged to
//! `$XDG_STATE_HOME/north/gates/unrunnable-box.jsonl`; once it enforces, the gh call is refused.
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

/// One issue a gh call writes: the issue number an edit names, none for a create, and the body the issue holds before the edit.
pub struct Filing<'a> {
    pub issue: Option<&'a str>,
    pub held: Option<&'a str>,
}

pub trait Log {
    fn enforcing(&self) -> bool;
    fn record(&mut self, row: serde_json::Value);
}

pub struct Live;

impl Log for Live {
    fn enforcing(&self) -> bool {
        north_base::platform::harness::document("harness/gates.toml")["gate"][GATE]["mode"].as_str() == Some("enforce")
    }

    fn record(&mut self, row: serde_json::Value) {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let Some(state) = var("XDG_STATE_HOME").or_else(|| var("HOME").map(|home| home.join(".local/state"))) else {
            return;
        };
        let directory = state.join("north/gates");
        let _ = std::fs::create_dir_all(&directory);
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(format!("{GATE}.jsonl"))) {
            let _ = writeln!(file, "{row}");
        }
    }
}

/// Logs each new unrunnable box of `body` in `repo` while the gate is in shadow, and refuses the gh call naming them once it enforces.
pub fn judge(repo: &str, filings: &[Filing], body: &str, log: &mut impl Log) -> Result<(), String> {
    if !repo.to_lowercase().starts_with(OWNER) {
        return Ok(());
    }
    let enforcing = log.enforcing();
    let session = std::env::var(crate::fleet::runtime::SESSION_ENV).unwrap_or_default();
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let mut refused = Vec::new();
    for filing in filings {
        let held: BTreeSet<String> = filing.held.unwrap_or_default().lines().filter_map(|line| UNTICKED.captures(line)).map(|found| identity(&found[1])).collect();
        for text in unrunnable(body).into_iter().filter(|text| !held.contains(&identity(text))) {
            if !enforcing {
                log.record(serde_json::json!({"gate": GATE, "at": at, "would_refuse": true, "verb": if filing.issue.is_some() { "edit" } else { "create" }, "repo": repo, "issue": filing.issue, "session": session, "box": text}));
            }
            refused.push(text);
        }
    }
    if enforcing && !refused.is_empty() {
        return Err(refusal(&refused));
    }
    Ok(())
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

/// What names a box across edits of its issue: its opening words, since an edit that adds a `Measured` line or an evidence note to a held box rewrites the rest.
fn identity(text: &str) -> String {
    text.trim().chars().take(IDENTITY_CHARS).collect()
}

/// The text of each unticked box of `body` naming no backticked `Done when:` command and not saying it waits on `Needs:` or a date.
fn unrunnable(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| UNTICKED.captures(line).map(|found| found[1].trim().to_owned()))
        .filter(|text| !RUNNABLE.is_match(text) && !EXEMPT.is_match(text))
        .collect()
}

#[cfg(test)]
#[path = "unrunnable_box_tests.rs"]
mod tests;
