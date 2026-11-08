use crate::hook::{HookInput, Verdict};
use crate::platform::process;
use crate::platform::pyjson::Value;
use regex::Regex;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

const SOURCE: &[&str] = &[
    ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".rs", ".c", ".h",
];
const HEADER: &str = "code-health: this edit adds structural debt; fix it now (split the file or function, or move the import behind an allowed module) using the program-craftsmanship skill:";
const HABIT_WARNING: &str = "habits: fix these before your next commit; they have no mechanical fix, and safe-push refuses a lane that still has one:";
const MAX_LINES: usize = 20;
const MAX_FILES: usize = 8;

static PATCHED_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\*\*\* (?:Add File|Update File|Move to):[ \t\r\n]*(.+?)[ \t\r\n]*$").unwrap()
});

pub fn evaluate(input: &HookInput) -> Verdict {
    if !matches!(input.tool_name(), "Edit" | "Write" | "MultiEdit" | "apply_patch") {
        return Verdict::Allow;
    }
    let cwd = match input.data["cwd"].as_str() {
        Some(cwd) => PathBuf::from(cwd),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let paths = source_files(input, &cwd);
    if paths.is_empty() || crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let limit = limit();
    let (mut lines, mut habits, mut notes) = (Vec::new(), Vec::new(), Vec::new());
    for path in paths.iter().take(MAX_FILES) {
        let (debt, edit_habits, headroom) = findings(path, &cwd, limit);
        lines.extend(debt);
        habits.extend(edit_habits);
        if let Some(note) = headroom
            && !notes.contains(&note)
        {
            notes.push(note);
        }
    }
    if !lines.is_empty() {
        lines.truncate(MAX_LINES);
        notes.insert(0, format!("{HEADER}\n{}", lines.join("\n")));
    }
    notes.splice(0..0, habits);
    if notes.is_empty() { Verdict::Allow } else { Verdict::Advise(notes.join("\n")) }
}

fn limit() -> Duration {
    std::env::var("CODE_HEALTH_EDIT_TIMEOUT")
        .ok()
        .and_then(|seconds| seconds.trim().parse::<f64>().ok())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .unwrap_or(Duration::from_secs(1))
}

fn edited_paths(input: &HookInput) -> Vec<String> {
    let text = |key: &str| input.tool_input[key].as_str().unwrap_or_default().to_owned();
    if input.tool_name() != "apply_patch" {
        let first = text("file_path");
        return vec![if first.is_empty() { text("filePath") } else { first }];
    }
    let patch = match input.tool_input.as_str() {
        Some(text) => text.to_owned(),
        None => ["command", "input", "patch"]
            .iter()
            .map(|key| match input.tool_input[*key].as_array() {
                Some(parts) => parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
                None => text(key),
            })
            .find(|text| !text.is_empty())
            .unwrap_or_default(),
    };
    PATCHED_FILE.captures_iter(&patch).map(|found| found[1].to_owned()).collect()
}

fn source_files(input: &HookInput, cwd: &Path) -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut paths: Vec<PathBuf> = Vec::new();
    for item in edited_paths(input).into_iter().filter(|item| !item.is_empty()) {
        let expanded = match item.strip_prefix("~/") {
            Some(rest) if !home.is_empty() => format!("{home}/{rest}"),
            _ if item == "~" && !home.is_empty() => home.clone(),
            _ => item,
        };
        let path = normalize(&cwd.join(expanded));
        let named = path.to_string_lossy();
        if SOURCE.iter().any(|extension| named.ends_with(extension)) && path.is_file() && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn normalize(path: &Path) -> PathBuf {
    let mut kept: Vec<Component> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match kept.last() {
                Some(Component::Normal(_)) => {
                    kept.pop();
                }
                Some(Component::RootDir) => {}
                _ => kept.push(part),
            },
            _ => kept.push(part),
        }
    }
    kept.iter().collect()
}

fn run(program: &str, args: &[&std::ffi::OsStr], dir: &Path, limit: Duration) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(dir);
    let output = process::run_group_within(command, limit)?;
    matches!(output.status.code(), Some(0 | 1)).then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn findings(path: &Path, cwd: &Path, limit: Duration) -> (Vec<String>, Vec<String>, Option<String>) {
    let Some(parent) = path.parent() else {
        return (Vec::new(), Vec::new(), None);
    };
    let args = ["-C".as_ref(), parent.as_os_str(), "rev-parse".as_ref(), "--show-toplevel".as_ref()];
    let Some(top) = run("git", &args, cwd, limit) else {
        return (Vec::new(), Vec::new(), None);
    };
    let top = PathBuf::from(top.trim());
    if top.as_os_str().is_empty() || !top.join("structure.toml").is_file() {
        return (Vec::new(), Vec::new(), None);
    }
    let lines: Vec<String> = run("north", &["code-health".as_ref(), "check".as_ref(), path.as_os_str()], &top, limit)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();
    let relative = path.strip_prefix(&top).ok().and_then(Path::to_str).filter(|name| name.ends_with(".rs"));
    let habits = relative.map(|relative| habit_notes(&top, relative)).unwrap_or_default();
    (lines, habits, relative.and_then(|relative| crate::code_health::crate_headroom(&top, relative)))
}

fn habit_notes(top: &Path, relative: &str) -> Vec<String> {
    let mut notes: Vec<String> = crate::code_health::habits_fix(top, relative)
        .iter()
        .map(|rewrite| {
            format!(
                "habits: {} rewrote {}:{} for you. Before: `{}` After: `{}` Write this form next time.",
                rewrite.rule, rewrite.path, rewrite.line, rewrite.before, rewrite.after
            )
        })
        .collect();
    let open = crate::code_health::habits(top, relative);
    if !open.is_empty() {
        notes.push(format!("{HABIT_WARNING}\n{}", open.join("\n")));
    }
    notes
}

#[cfg(test)]
#[path = "code_health_edit_tests.rs"]
mod tests;
