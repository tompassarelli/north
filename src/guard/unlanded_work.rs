use crate::hook::{HookInput, Verdict};
use crate::platform::{archive_tags, process};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

const STALE_HOURS: f64 = 24.0;
const FLEET_LIMIT: Duration = Duration::from_secs(5);

static TASK_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:tompassarelli/)?([\w.-]+)#(\d+)$").unwrap());

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let mut text = stale_lanes_notice(&report_path(&home), &home).unwrap_or_default();
    if let Some((repo, number)) = session_item(input) {
        text.push_str(&archive_tags::report(&repo, number, input.data["cwd"].as_str().map(Path::new)));
    }
    match text.trim_end() {
        "" => Verdict::Allow,
        notice => Verdict::Advise(notice.to_owned()),
    }
}

fn report_path(home: &str) -> PathBuf {
    std::env::var_os("LANE_GC_UNLANDED")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(home).join(".local/state/agents/lane-gc/unlanded.txt"))
}

fn leading_number(field: &str) -> f64 {
    let end = field.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(field.len());
    field[..end].parse().unwrap_or(0.0)
}

pub(super) fn stale_lanes_notice(report: &Path, home: &str) -> Option<String> {
    let contents = std::fs::read(report).ok()?;
    let count = String::from_utf8_lossy(&contents)
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter(|line| leading_number(line.split('\t').next().unwrap_or("")) >= STALE_HOURS)
        .count();
    if count == 0 {
        return None;
    }
    let shown = report.to_string_lossy();
    let shown = match shown.strip_prefix(home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => shown.into_owned(),
    };
    Some(format!(
        "{count} unlanded worktrees/branches older than 24 h are listed in {shown} (lane-gc --unlanded). Before starting an issue, continue an existing lane that references it instead of starting over.\n"
    ))
}

fn session_item(input: &HookInput) -> Option<(String, u64)> {
    let session = input.data["session_id"].as_str().filter(|s| !s.is_empty())?;
    let north = std::env::var("NORTH_BIN").unwrap_or_else(|_| "north".to_owned());
    let mut command = Command::new(north);
    command.args(["fleet", "list", "--json"]);
    let output = process::run_command_within(command, None, FLEET_LIMIT)?;
    if !output.status.success() {
        return None;
    }
    let nodes: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let node = nodes.as_array()?.iter().find(|node| {
        node["conversation"] == session && node["status"] != "exited"
    })?;
    let found = TASK_REF.captures(node["task_ref"].as_str()?)?;
    Some((archive_tags::repo_alias(&found[1]).to_owned(), found[2].parse().ok()?))
}
