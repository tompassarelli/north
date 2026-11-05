use crate::hook::{HookInput, Verdict};
use crate::platform::{process, repo_names};
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
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let cwd = input.data["cwd"].as_str().map(Path::new);
    let item = session_item(input);
    let scope = Scope {
        home: &home,
        container: cwd.and_then(|cwd| container(cwd, &home)),
        item: item.as_ref().map(|(repo, number)| (repo.as_str(), *number)),
    };
    let text = stale_lanes_notice(&report_path(&home), &scope).unwrap_or_default();
    match text.trim_end() {
        "" => Verdict::Allow,
        notice => Verdict::Advise(notice.to_owned()),
    }
}

pub(super) struct Scope<'a> {
    pub home: &'a str,
    pub container: Option<&'a str>,
    pub item: Option<(&'a str, u64)>,
}

pub(super) fn container<'a>(cwd: &'a Path, home: &str) -> Option<&'a str> {
    let rest = cwd.strip_prefix(Path::new(home).join("code")).ok()?;
    rest.components().next()?.as_os_str().to_str()
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

fn references(refs: &str, container: &str, (repo, number): (&str, u64)) -> bool {
    refs.split_whitespace().any(|reference| match reference.split_once('#') {
        Some((named, n)) if n.parse() == Ok(number) => {
            let named = named.strip_prefix("tompassarelli/").unwrap_or(named);
            let named = if named.is_empty() { container } else { named };
            repo_names::repo_alias(named) == repo
        }
        _ => false,
    })
}

const SHOWN_LANES: usize = 2;

pub(super) fn stale_lanes_notice(report: &Path, scope: &Scope) -> Option<String> {
    let container = scope.container?;
    let contents = std::fs::read(report).ok()?;
    let contents = String::from_utf8_lossy(&contents);
    let lanes: Vec<Vec<&str>> = contents
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|fields| fields.get(1) == Some(&container))
        .filter(|fields| leading_number(fields[0]) >= STALE_HOURS)
        .collect();
    if lanes.is_empty() {
        return None;
    }
    let shown = report.to_string_lossy();
    let shown = match shown.strip_prefix(scope.home) {
        Some(rest) if !scope.home.is_empty() => format!("~{rest}"),
        _ => shown.into_owned(),
    };
    let mut notice = format!(
        "{} unlanded {container} lanes older than 24 h are listed in {shown} (north lane-gc --unlanded). Before starting an issue, continue an existing lane that references it instead of starting over.\n",
        lanes.len()
    );
    if let Some(item @ (repo, number)) = scope.item {
        let named: Vec<&str> = lanes
            .iter()
            .filter(|fields| references(fields.get(7).copied().unwrap_or(""), container, item))
            .map(|fields| match fields.get(2).copied() {
                Some(path) if path != "(branch)" => path,
                _ => fields.get(3).copied().unwrap_or(""),
            })
            .collect();
        if !named.is_empty() {
            let more = named.len().saturating_sub(SHOWN_LANES);
            let tail = if more > 0 { format!(" and {more} more") } else { String::new() };
            notice.push_str(&format!("Lanes referencing {repo}#{number}: {}{tail}.\n", named[..named.len().min(SHOWN_LANES)].join(", ")));
        }
    }
    Some(notice)
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
    Some((repo_names::repo_alias(&found[1]).to_owned(), found[2].parse().ok()?))
}
