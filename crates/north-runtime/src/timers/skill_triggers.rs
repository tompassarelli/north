use std::path::Path;
use std::process::Command;

use crate::foundation::clock::Timestamp;

pub(super) const DEFAULT_CADENCE_DAYS: i64 = 30;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Skill {
    pub id: String,
    pub grounded_day: Option<i64>,
    pub grounded: String,
    pub cadence: Option<i64>,
    pub versions: Vec<(String, String)>,
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Reason {
    Cadence { days: i64, cadence: i64 },
    Version { tool: String, stamped: String, installed: String },
    Source { source: String },
}

pub(super) trait VersionProbe {
    fn version(&self, tool: &str) -> Option<String>;
}

pub(super) trait SourceChanges {
    fn changed_since(&self, source: &str, grounded: &str) -> bool;
}

pub(super) fn parse(id: &str, text: &str) -> Skill {
    let mut skill = Skill { id: id.to_owned(), ..Skill::default() };
    let Some(front) = text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---").map(|(front, _)| front)) else {
        return skill;
    };
    for line in front.lines().filter(|line| !line.starts_with([' ', '\t'])) {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match key.trim() {
            "grounded" => {
                skill.grounded = value.to_owned();
                skill.grounded_day = Timestamp::parse_loose(value).map(Timestamp::unix_days);
            }
            "cadence" => skill.cadence = value.parse().ok().filter(|days| *days > 0),
            "sources" => {
                skill.sources = value.trim_matches(['[', ']']).split(',').map(|item| item.trim().trim_matches(['"', '\'']).to_owned()).filter(|item| !item.is_empty()).collect();
            }
            key => {
                if let Some(tool) = key.strip_suffix("-version").filter(|tool| !tool.is_empty()) {
                    skill.versions.push((tool.to_owned(), value.trim_matches(['"', '\'']).to_owned()));
                }
            }
        }
    }
    skill
}

pub(super) fn reasons(skill: &Skill, today_day: i64, probe: &dyn VersionProbe, sources: &dyn SourceChanges) -> Vec<Reason> {
    let mut found = Vec::new();
    if let Some(grounded) = skill.grounded_day {
        let days = today_day - grounded;
        let cadence = skill.cadence.unwrap_or(DEFAULT_CADENCE_DAYS);
        if days >= cadence {
            found.push(Reason::Cadence { days, cadence });
        }
    }
    for (tool, stamped) in &skill.versions {
        if let Some(installed) = probe.version(tool).filter(|installed| !matches_stamp(installed, stamped)) {
            found.push(Reason::Version { tool: tool.clone(), stamped: stamped.clone(), installed });
        }
    }
    if skill.grounded_day.is_some() {
        for source in skill.sources.iter().filter(|source| sources.changed_since(source, &skill.grounded)) {
            found.push(Reason::Source { source: source.clone() });
        }
    }
    found
}

fn matches_stamp(installed: &str, stamped: &str) -> bool {
    installed == stamped || installed.strip_prefix(stamped).is_some_and(|rest| rest.starts_with('.'))
}

pub(super) fn line(skill: &Skill, reason: &Reason) -> String {
    let what = match reason {
        Reason::Cadence { days, cadence } => format!("grounded (last {}, {days} days, cadence {cadence})", skill.grounded),
        Reason::Version { tool, stamped, installed } => format!("{tool} version drift (stamped {stamped}, installed {installed})"),
        Reason::Source { source } => format!("source `{source}` changed since grounded {}", skill.grounded),
    };
    format!("- [ ] `{}` {what}", skill.id)
}

pub(super) fn lines(skills: &[Skill], today_day: i64, probe: &dyn VersionProbe, sources: &dyn SourceChanges) -> Vec<String> {
    skills.iter().flat_map(|skill| reasons(skill, today_day, probe, sources).into_iter().map(move |reason| line(skill, &reason))).collect()
}

pub(super) fn load(directory: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(directory) else { return Vec::new() };
    let mut skills: Vec<Skill> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(entry.path().join("SKILL.md")).ok()?;
            Some(parse(&id, &text))
        })
        .collect();
    skills.sort_by(|a, b| a.id.cmp(&b.id));
    skills
}

pub(super) fn version_token(output: &str) -> Option<String> {
    output
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|word| word.trim_start_matches('v'))
        .find(|word| word.contains('.') && word.chars().next().is_some_and(|c| c.is_ascii_digit()) && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
        .map(str::to_owned)
}

pub(super) struct HostProbe;

impl VersionProbe for HostProbe {
    fn version(&self, tool: &str) -> Option<String> {
        let output = Command::new(tool).arg("--version").output().ok().filter(|output| output.status.success())?;
        version_token(&String::from_utf8_lossy(&output.stdout))
    }
}

pub(super) struct RepositorySources<'a> {
    pub north: &'a Path,
    pub watch_toml: &'a Path,
}

impl SourceChanges for RepositorySources<'_> {
    fn changed_since(&self, source: &str, grounded: &str) -> bool {
        self.watch_reviewed_after(source, grounded) || self.reference_changed_after(source, grounded)
    }
}

impl RepositorySources<'_> {
    fn watch_reviewed_after(&self, source: &str, grounded: &str) -> bool {
        let Ok(text) = std::fs::read_to_string(self.watch_toml) else { return false };
        let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return false };
        let Some(tables) = document.get("source").and_then(toml_edit::Item::as_array_of_tables) else { return false };
        let Some(grounded) = Timestamp::parse_loose(grounded).map(Timestamp::unix_days) else { return false };
        tables.iter().filter(|table| table.get("id").and_then(toml_edit::Item::as_str) == Some(source)).any(|table| {
            table
                .get("last_reviewed")
                .and_then(toml_edit::Item::as_value)
                .and_then(toml_edit::Value::as_datetime)
                .map(ToString::to_string)
                .and_then(|reviewed| Timestamp::parse_loose(&reviewed))
                .is_some_and(|reviewed| reviewed.unix_days() > grounded)
        })
    }

    fn reference_changed_after(&self, anchor: &str, grounded: &str) -> bool {
        if !anchor.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return false;
        }
        let range = format!("/<a id=\"{anchor}\">/,/^- <a id=/:docs/references.md");
        Command::new("git")
            .arg("-C")
            .arg(self.north)
            .args(["log", "--format=%H", "-L"])
            .arg(range)
            .arg(format!("--after={grounded}T23:59:59"))
            .output()
            .ok()
            .filter(|output| output.status.success())
            .is_some_and(|output| !output.stdout.is_empty())
    }
}
