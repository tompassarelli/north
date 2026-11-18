use crate::hook_input::{HookInput, Verdict};
use std::path::{Path, PathBuf};

const STALE_HOURS: f64 = 24.0;

pub fn evaluate(input: &HookInput) -> Verdict {
    let home = std::env::var("HOME").unwrap_or_default();
    let cwd = input.data["cwd"].as_str().map(Path::new);
    let scope = Scope { home: &home, container: cwd.and_then(|cwd| container(cwd, &home)) };
    let text = stale_lanes_notice(&report_path(&home), &scope).unwrap_or_default();
    match text.trim_end() {
        "" => Verdict::Allow,
        notice => Verdict::Advise(notice.to_owned()),
    }
}

pub(super) struct Scope<'a> {
    pub home: &'a str,
    pub container: Option<&'a str>,
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
    Some(format!(
        "{} unlanded {container} lanes older than 24 h are listed in {shown} (north lane gc --unlanded). Before starting an issue, continue an existing lane that references it instead of starting over.\n",
        lanes.len()
    ))
}
