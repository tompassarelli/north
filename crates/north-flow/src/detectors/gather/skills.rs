//! What the skill-review detector reads: each skill's last review date from its frontmatter, the commits that changed
//! its files and the corrections classed to it since, and the drift of a stamped tool version or a cited source.
use std::collections::BTreeMap;
use std::fs;

use super::Gatherer;
use crate::detectors::skill_review::{Facts, Skill};
use crate::detectors::default_text;
use crate::foundation::clock::Timestamp;
use crate::threads::incidents::corrections;

const SKILLS: &str = "harness/skills";
const CHANGE_HISTORY: &str = "--since=120 days ago";
const DAY_S: f64 = 86_400.0;
const SOURCE_REPO: &str = "south";
const WATCH_TOML: &str = "agents/watch.toml";

#[derive(Default)]
struct Frontmatter {
    reviewed: Option<String>,
    grounded: Option<String>,
    versions: Vec<(String, String)>,
    sources: Vec<String>,
}

fn later(current: Option<String>, date: &str) -> Option<String> {
    Timestamp::parse_loose(date).map(|_| date.to_owned()).into_iter().chain(current).max()
}

fn frontmatter(text: &str) -> Frontmatter {
    let mut found = Frontmatter::default();
    let Some(front) = text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---").map(|(front, _)| front)) else { return found };
    for line in front.lines().filter(|line| !line.starts_with([' ', '\t'])) {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match key.trim() {
            "grounded" => {
                found.grounded = Some(value.to_owned());
                found.reviewed = later(found.reviewed, value);
            }
            "written" => found.reviewed = later(found.reviewed, value),
            "sources" => found.sources = value.trim_matches(['[', ']']).split(',').map(|item| item.trim().trim_matches(['"', '\'']).to_owned()).filter(|item| !item.is_empty()).collect(),
            key => {
                if let Some(tool) = key.strip_suffix("-version").filter(|tool| !tool.is_empty()) {
                    found.versions.push((tool.to_owned(), value.trim_matches(['"', '\'']).to_owned()));
                }
            }
        }
    }
    found
}

fn version_token(output: &str) -> Option<String> {
    output
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|word| word.trim_start_matches('v'))
        .find(|word| word.contains('.') && word.chars().next().is_some_and(|c| c.is_ascii_digit()) && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
        .map(str::to_owned)
}

fn matches_stamp(installed: &str, stamped: &str) -> bool {
    installed == stamped || installed.strip_prefix(stamped).is_some_and(|rest| rest.starts_with('.'))
}

fn day_end(date: &str) -> Option<f64> {
    Timestamp::parse_loose(date).map(|day| (day.unix_days() + 1) as f64 * DAY_S)
}

impl Gatherer<'_> {
    fn skill_changes(&self) -> BTreeMap<String, Vec<(f64, String)>> {
        let log = self.git(&["log", "origin/main", CHANGE_HISTORY, "--name-only", "--format=%x01%ct%x09%s", "--", SKILLS]).unwrap_or_default();
        let mut changes: BTreeMap<String, Vec<(f64, String)>> = BTreeMap::new();
        for commit in log.split('\u{1}').filter(|commit| !commit.trim().is_empty()) {
            let mut lines = commit.lines();
            let Some((at, subject)) = lines.next().and_then(|head| head.split_once('\t')) else { continue };
            let Ok(at) = at.trim().parse::<f64>() else { continue };
            let skills: std::collections::BTreeSet<&str> = lines.filter_map(|path| path.strip_prefix(SKILLS)?.strip_prefix('/')?.split('/').next()).collect();
            for skill in skills {
                changes.entry(skill.to_owned()).or_default().push((at, subject.chars().take(80).collect()));
            }
        }
        changes
    }

    fn reference_changed(&self, anchor: &str, grounded: &str) -> bool {
        if !anchor.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return false;
        }
        let range = format!("/<a id=\"{anchor}\">/,/^- <a id=/:docs/references.md");
        self.git(&["log", "--format=%H", "-L", &range, &format!("--after={grounded}T23:59:59")]).is_some_and(|log| !log.trim().is_empty())
    }

    fn watch_reviewed(&self, source: &str, grounded: &str) -> bool {
        let Ok(text) = fs::read_to_string(self.places.checkout(SOURCE_REPO).join(WATCH_TOML)) else { return false };
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

    fn skill_drift(&self, front: &Frontmatter) -> Vec<String> {
        let mut drift = Vec::new();
        for (tool, stamped) in &front.versions {
            let installed = self.host.run(&[tool.as_str(), "--version"], None, None).and_then(|output| version_token(&output));
            if let Some(installed) = installed.filter(|installed| !matches_stamp(installed, stamped)) {
                drift.push(format!("{tool} version drift (stamped {stamped}, installed {installed})"));
            }
        }
        if let Some(grounded) = &front.grounded {
            drift.extend(front.sources.iter().filter(|source| self.watch_reviewed(source, grounded) || self.reference_changed(source, grounded)).map(|source| format!("source `{source}` changed since grounded {grounded}")));
        }
        drift
    }

    pub fn skills(&self) -> Facts {
        let mut changes = self.skill_changes();
        let seen = self.corrections().seen;
        let mut directories: Vec<_> = fs::read_dir(self.checkout().join(SKILLS)).into_iter().flatten().flatten().collect();
        directories.sort_by_key(fs::DirEntry::file_name);
        let skills = directories
            .into_iter()
            .filter_map(|entry| {
                let id = entry.file_name().to_string_lossy().into_owned();
                let front = frontmatter(&fs::read_to_string(entry.path().join("SKILL.md")).ok()?);
                let reviewed = front.reviewed.clone()?;
                let after = day_end(&reviewed)?;
                let changed = changes.remove(&id).unwrap_or_default().into_iter().filter(|(at, _)| *at >= after).map(|(_, subject)| subject).collect();
                let corrected = seen.iter().filter(|(class, at)| *at >= after && corrections::named(class).and_then(|class| class.skill.as_deref()) == Some(&id)).map(|(_, at)| Timestamp::from_unix_seconds_f64(*at).iso()).collect();
                Some(Skill { drift: self.skill_drift(&front), id, reviewed, changes: changed, corrections: corrected })
            })
            .collect();
        Facts { owner: Some(default_text("detector_skill_review_owner")), skills }
    }
}
