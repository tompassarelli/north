use super::handoff_record::edited_paths;
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use toml_edit::DocumentMut;

const TABLE: &str = include_str!("../../agent-machinery/file-skills.toml");
const PROFILE_SECTION: &str = "## Profiles for dev and test";
const PROFILE_BULLETS: usize = 3;
const PROFILE_FILE: &str = "references/compile-times.md";

static WORKSPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*\[workspace\]").unwrap());
static PACKAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*\[package\]").unwrap());
static DEV_PROFILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*\[profile\.dev\]").unwrap());

struct Rule {
    skill: String,
    extensions: Vec<String>,
    names: Vec<String>,
    package: Option<String>,
}

pub struct Roots {
    pub skills: PathBuf,
    pub seen: PathBuf,
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    match roots() {
        Some(roots) => decide(input, &roots),
        None => Verdict::Allow,
    }
}

fn roots() -> Option<Roots> {
    let get = |name: &str| std::env::var(name).ok().filter(|s| !s.is_empty());
    let home = get("HOME").unwrap_or_default();
    let current = crate::hook::activation_paths(&get).0.parent()?.to_owned();
    let state = get("XDG_STATE_HOME").unwrap_or_else(|| format!("{home}/.local/state"));
    Some(Roots { skills: current.join("skills/shared"), seen: Path::new(&state).join("north/file-skills") })
}

pub(super) fn decide(input: &HookInput, roots: &Roots) -> Verdict {
    let Some(session) = input.data["session_id"].as_str().filter(|s| !s.is_empty()) else {
        return Verdict::Allow;
    };
    let cwd = input.data["cwd"].as_str().map(PathBuf::from).unwrap_or_default();
    let rules = rules();
    let mut notes = Vec::new();
    for item in edited_paths(input).into_iter().filter(|p| !p.is_empty()) {
        let path = cwd.join(item);
        for rule in rules.iter().filter(|rule| rule.matches(&path)) {
            let dir = roots.skills.join(&rule.skill);
            if let Some(body) = body(&dir)
                && claim(&roots.seen, session, &rule.skill)
            {
                notes.push(format!("file-skill: editing {} loads the {} skill (directory {}); its rules apply to this edit and the rest of the session.\n\n{body}", path.display(), rule.skill, dir.display()));
            }
            if rule.skill == "rust-development"
                && let Some(note) = missing_dev_profile(input, &path, &dir, roots, session)
            {
                notes.push(note);
            }
        }
    }
    match notes.is_empty() {
        true => Verdict::Allow,
        false => Verdict::Advise(notes.join("\n\n")),
    }
}

fn rules() -> Vec<Rule> {
    let Ok(doc) = TABLE.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let list = |table: &toml_edit::Table, key: &str| -> Vec<String> {
        table.get(key).and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()).map(str::to_owned).collect()
    };
    doc["rule"]
        .as_array_of_tables()
        .into_iter()
        .flatten()
        .filter_map(|table| {
            Some(Rule {
                skill: table.get("skill")?.as_str()?.to_owned(),
                extensions: list(table, "extensions"),
                names: list(table, "names"),
                package: table.get("package").and_then(|v| v.as_str()).map(str::to_owned),
            })
        })
        .collect()
}

impl Rule {
    fn matches(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        (self.names.iter().any(|n| n == name) || self.extensions.iter().any(|e| e == extension))
            && self.package.as_deref().is_none_or(|package| installed(path, package))
    }
}

fn installed(path: &Path, package: &str) -> bool {
    for dir in path.ancestors().skip(1) {
        let declared = std::fs::read(dir.join("package.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|json| ["dependencies", "devDependencies", "peerDependencies"].iter().any(|k| json[k].get(package).is_some()));
        if declared || dir.join("node_modules").join(package).is_dir() {
            return true;
        }
        if dir.join(".git").exists() {
            return false;
        }
    }
    false
}

fn body(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let text = text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---\n")).map_or(text.as_str(), |(_, body)| body);
    Some(text.trim().replace("](references/", &format!("]({}/references/", dir.display())))
}

fn claim(seen: &Path, session: &str, key: &str) -> bool {
    let session: String = session.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let dir = seen.join(session);
    let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    std::fs::create_dir_all(&dir).is_ok() && OpenOptions::new().write(true).create_new(true).open(dir.join(&hash[..24])).is_ok()
}

fn edited_text(input: &HookInput) -> String {
    let tool = &input.tool_input;
    let edits = tool["edits"].as_array().into_iter().flatten().filter_map(|e| e["new_string"].as_str());
    let patch = ["command", "input", "patch"].iter().filter_map(|k| tool[*k].as_str()).chain(tool.as_str()).flat_map(str::lines).filter_map(|line| line.strip_prefix('+'));
    let direct = ["content", "new_string"].iter().filter_map(|k| tool[*k].as_str()).chain(edits);
    direct.chain(patch).collect::<Vec<_>>().join("\n")
}

fn missing_dev_profile(input: &HookInput, edited: &Path, skill: &Path, roots: &Roots, session: &str) -> Option<String> {
    let manifest = |dir: &Path| {
        let path = dir.join("Cargo.toml");
        let disk = std::fs::read_to_string(&path).unwrap_or_default();
        let text = if path == edited { format!("{disk}\n{}", edited_text(input)) } else { disk };
        (path.is_file() || path == edited).then_some((path, text))
    };
    let mut package: Option<(PathBuf, String)> = None;
    let root = edited.ancestors().skip(1).filter_map(manifest).find(|(path, text)| {
        let workspace = WORKSPACE.is_match(text);
        if !workspace && package.is_none() && PACKAGE.is_match(text) {
            package = Some((path.clone(), text.clone()));
        }
        workspace
    });
    let (path, text) = root.or(package)?;
    if DEV_PROFILE.is_match(&text) {
        return None;
    }
    let reference = std::fs::read_to_string(skill.join(PROFILE_FILE)).ok()?;
    let mut bullets = 0;
    let lines: Vec<&str> = reference
        .lines()
        .skip_while(|line| *line != PROFILE_SECTION)
        .skip(1)
        .take_while(|line| {
            bullets += line.starts_with("- **") as usize;
            bullets < PROFILE_BULLETS && !line.starts_with("## ")
        })
        .collect();
    let key = format!("dev-profile {}", path.display());
    if lines.is_empty() || !claim(&roots.seen, session, &key) {
        return None;
    }
    Some(format!("file-skill: this Cargo workspace has no [profile.dev] section, so it builds on toolchain defaults (full debuginfo, unoptimised dependencies). Add one now, measured per the compile-times reference ({}/{PROFILE_FILE}):\n{}", skill.display(), lines.join("\n").trim()))
}

#[cfg(test)]
pub(super) fn rules_len() -> usize {
    rules().len()
}
