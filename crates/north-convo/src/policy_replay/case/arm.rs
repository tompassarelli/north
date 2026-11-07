use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

use super::source::Case;
use crate::error::NorthError;
use crate::foundation::clock::Timestamp;

const OWN: &str = "own";
const CURRENT: &str = "current";
const ALLOWED: [&str; 5] = ["Read", "Grep", "Glob", "Skill", "ToolSearch"];

pub struct Arm {
    pub name: String,
    pub spec: String,
}

pub struct Generation {
    pub dir: PathBuf,
    texts: BTreeMap<String, String>,
}

fn refusal(message: String) -> NorthError {
    NorthError::Usage(format!("policy-replay case: {message}"))
}

fn generations_root() -> PathBuf {
    std::env::var_os("NORTH_AGENT_GENERATIONS").map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/north-data/agents"), PathBuf::from)
}

pub fn parse(spec: &str) -> Arm {
    match spec.split_once('=') {
        Some((name, generation)) => Arm { name: name.to_owned(), spec: generation.to_owned() },
        None => Arm { name: spec.to_owned(), spec: spec.to_owned() },
    }
}

fn rendered(text: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|line| !(line.trim_start().starts_with("<!--") && line.trim_end().ends_with("-->"))).collect();
    kept.join("\n").trim().to_owned()
}

fn front_matter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((&rest[..end], &rest[end + 5..]))
}

fn description(header: &str) -> Option<String> {
    let mut lines = header.lines().skip_while(|line| !line.starts_with("description:"));
    let first = lines.next()?.trim_start_matches("description:").trim();
    if !first.starts_with('>') && !first.starts_with('|') {
        return Some(first.trim_matches(|c| c == '"' || c == '\'').to_owned());
    }
    let folded: Vec<&str> = lines.take_while(|line| line.starts_with(' ')).map(str::trim).collect();
    Some(folded.join(" "))
}

fn walk(root: &Path, dir: &Path, texts: &mut BTreeMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, texts);
        } else if let (Ok(text), Ok(rel)) = (fs::read_to_string(&path), path.strip_prefix(root)) {
            texts.insert(format!("skills/{}", rel.display()), text);
        }
    }
}

impl Generation {
    pub fn load(dir: PathBuf) -> Result<Self, NorthError> {
        let instructions = fs::read_to_string(dir.join("instructions/shared/AGENTS.md")).map_err(|error| refusal(format!("{} is not an agent generation: {error}", dir.display())))?;
        let mut texts = BTreeMap::from([("CLAUDE.md".to_owned(), instructions)]);
        let skills = dir.join("skills/shared");
        walk(&skills, &skills, &mut texts);
        Ok(Self { dir, texts })
    }

    pub fn name(&self) -> String {
        self.dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn pieces(&self, key: &str) -> Vec<String> {
        let Some(text) = self.texts.get(key) else {
            return Vec::new();
        };
        let mut pieces = vec![rendered(text)];
        if let Some((header, body)) = front_matter(text) {
            pieces.push(rendered(body));
            pieces.extend(description(header));
        }
        pieces
    }

    pub fn substitutions(&self, arm: &Generation) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = self
            .texts
            .iter()
            .filter(|(key, text)| arm.texts.get(*key).is_some_and(|other| other != *text))
            .flat_map(|(key, _)| self.pieces(key).into_iter().zip(arm.pieces(key)))
            .filter(|(old, new)| old != new && old.len() > 40)
            .collect();
        pairs.sort_by_key(|(old, _)| std::cmp::Reverse(old.len()));
        pairs
    }
}

fn instructions_in(chain: &[Value]) -> Option<String> {
    chain
        .iter()
        .rev()
        .filter(|record| record["attachment"]["type"] == "instructions")
        .find_map(|record| record["attachment"]["files"].as_array()?.iter().find(|file| file["path"].as_str().is_some_and(|path| path.ends_with("CLAUDE.md")))?["content"].as_str().map(str::to_owned))
}

fn own(case: &Case) -> Result<PathBuf, NorthError> {
    let recorded = instructions_in(&case.chain).ok_or_else(|| refusal(format!("{} carries no CLAUDE.md before its cut", case.id)))?;
    let cut = Timestamp::parse_loose(&case.cut_at)
        .map(|stamp| UNIX_EPOCH + Duration::from_secs_f64(stamp.unix_seconds_f64()))
        .ok_or_else(|| refusal(format!("{} cut time {} is not a time", case.id, case.cut_at)))?;
    let mut built: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(generations_root())
        .map_err(NorthError::Io)?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("gen-"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .filter(|(modified, _)| *modified <= cut)
        .collect();
    built.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let wanted = rendered(&recorded);
    built
        .into_iter()
        .map(|(_, dir)| dir)
        .find(|dir| fs::read_to_string(dir.join("instructions/shared/AGENTS.md")).is_ok_and(|text| rendered(&text) == wanted))
        .ok_or_else(|| refusal(format!("no agent generation built before {} renders the CLAUDE.md that {} carries", case.cut_at, case.id)))
}

pub fn generation(spec: &str, case: &Case) -> Result<Generation, NorthError> {
    let dir = match spec {
        OWN => own(case)?,
        CURRENT => fs::canonicalize(generations_root().join(CURRENT)).map_err(NorthError::Io)?,
        path if Path::new(path).is_absolute() => PathBuf::from(path),
        name => generations_root().join(name),
    };
    Generation::load(dir)
}

fn substitute(value: &mut Value, pairs: &[(String, String)], count: &mut usize) {
    match value {
        Value::String(text) => {
            for (old, new) in pairs {
                let found = text.matches(old.as_str()).count();
                if found > 0 {
                    *text = text.replace(old.as_str(), new);
                    *count += found;
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, pairs, count)),
        Value::Object(fields) => fields.values_mut().for_each(|field| substitute(field, pairs, count)),
        _ => {}
    }
}

pub fn transcript(chain: &[Value], pairs: &[(String, String)]) -> (String, usize) {
    let mut count = 0;
    let lines: Vec<String> = chain
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if !pairs.is_empty() {
                substitute(&mut record, pairs, &mut count);
            }
            serde_json::to_string(&record).unwrap_or_default()
        })
        .collect();
    (lines.join("\n") + "\n", count)
}

pub fn config_dir(root: &Path, case: &Case, generation: &Generation, transcript: &str) -> Result<(), NorthError> {
    let project = root.join("projects").join(&case.project);
    fs::create_dir_all(&project).map_err(NorthError::Io)?;
    fs::write(project.join(format!("{}.jsonl", case.session)), transcript).map_err(NorthError::Io)?;
    fs::write(root.join("CLAUDE.md"), &generation.texts["CLAUDE.md"]).map_err(NorthError::Io)?;
    std::os::unix::fs::symlink(generation.dir.join("skills/shared"), root.join("skills")).map_err(NorthError::Io)?;
    let agents = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude/agents");
    if agents.exists() {
        std::os::unix::fs::symlink(fs::canonicalize(agents).map_err(NorthError::Io)?, root.join("agents")).map_err(NorthError::Io)?;
    }
    let hold = format!("{} policy-replay hold", std::env::current_exe().map_err(NorthError::Io)?.display());
    let settings = json!({
        "permissions": { "allow": ALLOWED },
        "hooks": { "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": hold }] }] },
    });
    fs::write(root.join("settings.json"), serde_json::to_string_pretty(&settings).map_err(NorthError::Json)?).map_err(NorthError::Io)
}

pub fn hold(input: &str) -> Option<String> {
    let call: Value = serde_json::from_str(input).ok()?;
    let tool = call["tool_name"].as_str()?;
    if ALLOWED.contains(&tool) {
        return None;
    }
    let reason = "Replay: this call was recorded and not executed, because this conversation replays a past turn to measure its harness text. Do not retry it, work around it or mention that it was not run; finish the turn exactly as you would have if it had run.";
    Some(json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason } }).to_string())
}
