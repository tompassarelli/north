use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const NORTH_RUNTIME: &str =
    r#"readlink -f "$(command -v north)" | sed -n 's|.*/runtimes/\([0-9a-f]\{40\}\)/.*|\1|p'"#;
const T3: &str = r#"t3="${T3_RUNTIME:-$HOME/.local/lib/t3/current/t3}""#;

/// The Claude Code binary itself: `claude` on a north host's path is a launcher that finds it through the caller's home.
pub fn claude_binary() -> Option<PathBuf> {
    let named = ["EVAL_T_CLAUDE", "CLAUDE_RUNTIME"]
        .into_iter()
        .find_map(env::var_os)
        .map(PathBuf::from);
    let installed = env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/lib/claude/current/bin/claude"));
    let on_path = || shell("command -v claude").map(PathBuf::from);
    let found = named
        .or_else(|| installed.filter(|path| path.exists()))
        .or_else(on_path)?;
    fs::canonicalize(found).ok()
}

pub fn runtime_sha() -> Option<String> {
    shell(NORTH_RUNTIME)
}

pub fn stamps(tree: &Path, claude: &Path) -> Value {
    json!({
        "north_tree": tree.display().to_string(),
        "north_runtime": runtime_sha(),
        "t3_version": shell(&format!(r#"{T3}; "$t3" --version"#)),
        "t3_sha256": sha256(&format!(r#"{T3}; sha256sum "$t3""#)),
        "claude_version": said(Command::new(claude).arg("--version")),
        "claude_sha256": said(Command::new("sha256sum").arg(claude)).and_then(|line| first_word(&line)),
    })
}

fn sha256(script: &str) -> Option<String> {
    shell(script).and_then(|line| first_word(&line))
}

fn first_word(line: &str) -> Option<String> {
    line.split_whitespace().next().map(str::to_owned)
}

fn shell(script: &str) -> Option<String> {
    said(Command::new("sh").args(["-c", script]))
}

fn said(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}
