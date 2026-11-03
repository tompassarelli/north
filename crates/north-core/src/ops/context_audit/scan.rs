use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::ops::convo::lines::{Source, scan_lines};

const LINE_CAP: usize = 64 << 20;
const CHUNK: usize = 1 << 20;
const GROUNDING: &str = "Grounding (auto-retrieved";
const FILE_SKILL: &str = "file-skill: editing ";
const SKILL_BASE: &str = "Base directory for this skill:";
const NOTIFICATION: &str = "<task-notification>";

static FILE_SKILL_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"loads the (\S+) skill").expect("file-skill expression"));
static SKILL_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/skills/([^/]+)/SKILL\.md$").expect("skill file expression"));
static REFERENCE_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/skills/([^/]+/references/.+)$").expect("reference expression"));
static NAMED_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[\s`'"(\[])((?:~/|/)?[A-Za-z0-9_.@-]+(?:/[A-Za-z0-9_.@-]+)+)"#).expect("path expression")
});
static REPO_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:~|/home/[^/]+)/code/[^/]+/(?:main|worktrees/[^/]+)/").expect("repo prefix expression")
});
static HOME_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/home/[^/]+/").expect("home prefix expression"));
static READ_VERB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s|;&(])(?:rg|grep|cat|sed|head|tail|less|bat|jq|wc)\s").expect("read verb expression"));

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Role {
    Lead,
    Worker,
}

impl Role {
    pub fn of(path: &Path) -> Self {
        if path.to_string_lossy().contains("/subagents/") { Self::Worker } else { Self::Lead }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Worker => "worker",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Kind {
    Startup,
    SkillBody,
    SkillReference,
    Grounding,
    OtherHook,
}

pub const KINDS: [Kind; 5] = [Kind::Startup, Kind::SkillBody, Kind::SkillReference, Kind::Grounding, Kind::OtherHook];

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::SkillBody => "skill_bodies",
            Self::SkillReference => "skill_references",
            Self::Grounding => "grounding",
            Self::OtherHook => "other_hooks",
        }
    }
}

#[derive(Default)]
pub struct Grounding {
    pub injections: usize,
    pub notification_injections: usize,
    pub named: usize,
    pub read: usize,
}

pub struct Audit {
    pub session: String,
    pub path: String,
    pub role: Role,
    pub chars: BTreeMap<Kind, usize>,
    pub skills: BTreeMap<String, usize>,
    pub references: BTreeMap<String, usize>,
    pub grounding: Grounding,
    pub oversized_lines: usize,
}

impl Audit {
    pub fn total(&self) -> usize {
        self.chars.values().sum()
    }
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    parent: Option<String>,
    #[serde(rename = "isMeta")]
    meta: Option<bool>,
    message: Option<Value>,
    attachment: Option<Value>,
}

enum Pending {
    Skill(String),
    Reference(String),
}

#[derive(Default)]
struct State {
    pending: HashMap<String, Pending>,
    notifications: HashSet<String>,
    injections: Vec<(usize, Vec<String>)>,
    accesses: Vec<(usize, String)>,
    step: usize,
}

pub fn audit_file(disk: &Path, logical: &Path, startup_chars: usize) -> std::io::Result<Audit> {
    let session = logical.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let mut audit = Audit {
        session,
        path: logical.to_string_lossy().into_owned(),
        role: Role::of(logical),
        chars: KINDS.iter().map(|kind| (*kind, 0)).collect(),
        skills: BTreeMap::new(),
        references: BTreeMap::new(),
        grounding: Grounding::default(),
        oversized_lines: 0,
    };
    audit.chars.insert(Kind::Startup, startup_chars);
    let mut state = State::default();
    let mut source = Source::open("zstd", &disk.to_string_lossy(), 0)?;
    scan_lines(&mut source, 0, CHUNK, LINE_CAP, |_, _, line| {
        match line {
            None => audit.oversized_lines += 1,
            Some(bytes) => {
                if let Ok(line) = serde_json::from_slice::<Line>(bytes) {
                    state.step += 1;
                    visit(&mut audit, &mut state, line);
                }
            }
        }
        Ok(())
    })?;
    source.close();
    settle_precision(&mut audit, &state);
    Ok(audit)
}

fn visit(audit: &mut Audit, state: &mut State, line: Line) {
    match line.kind.as_deref() {
        Some("attachment") => {
            if let Some(attachment) = &line.attachment {
                attachment_record(audit, state, attachment, line.uuid.as_deref(), line.parent.as_deref());
            }
        }
        Some("user") => {
            if let Some(message) = &line.message {
                user_record(audit, state, message, line.uuid.as_deref(), line.meta.unwrap_or(false));
            }
        }
        Some("assistant") => {
            if let Some(message) = &line.message {
                assistant_record(state, message);
            }
        }
        _ => {}
    }
}

fn joined(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().or_else(|| item.get("text").and_then(Value::as_str)))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn add_skill(audit: &mut Audit, name: &str, chars: usize) {
    *audit.chars.entry(Kind::SkillBody).or_default() += chars;
    *audit.skills.entry(name.to_owned()).or_default() += chars;
}

fn attachment_record(audit: &mut Audit, state: &mut State, attachment: &Value, uuid: Option<&str>, parent: Option<&str>) {
    match attachment.get("type").and_then(Value::as_str) {
        Some("queued_command") => {
            let prompt = joined(attachment.get("prompt"));
            if prompt.trim_start().starts_with(NOTIFICATION)
                && let Some(uuid) = uuid
            {
                state.notifications.insert(uuid.to_owned());
            }
        }
        Some("hook_additional_context") => {
            let text = joined(attachment.get("content"));
            let chars = text.chars().count();
            if text.starts_with(GROUNDING) {
                *audit.chars.entry(Kind::Grounding).or_default() += chars;
                audit.grounding.injections += 1;
                if parent.is_some_and(|parent| state.notifications.contains(parent)) {
                    audit.grounding.notification_injections += 1;
                }
                state.injections.push((state.step, named_paths(&text)));
            } else if let Some(name) = text.starts_with(FILE_SKILL).then(|| FILE_SKILL_NAME.captures(&text)).flatten() {
                let name = name[1].to_owned();
                add_skill(audit, &name, chars);
            } else {
                *audit.chars.entry(Kind::OtherHook).or_default() += chars;
            }
        }
        _ => {}
    }
}

fn user_record(audit: &mut Audit, state: &mut State, message: &Value, uuid: Option<&str>, meta: bool) {
    let content = message.get("content");
    let items: &[Value] = match content {
        Some(Value::Array(items)) => items,
        _ => &[],
    };
    let text = joined(content);
    if text.trim_start().starts_with(NOTIFICATION)
        && let Some(uuid) = uuid
    {
        state.notifications.insert(uuid.to_owned());
    }
    if meta && let Some(rest) = text.strip_prefix(SKILL_BASE) {
        let dir = rest.lines().next().unwrap_or("").trim().trim_end_matches('/');
        let name = dir.rsplit('/').next().unwrap_or(dir).to_owned();
        add_skill(audit, &name, text.chars().count());
        return;
    }
    for item in items.iter().filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_result")) {
        let Some(pending) = item.get("tool_use_id").and_then(Value::as_str).and_then(|id| state.pending.remove(id)) else { continue };
        let chars = joined(item.get("content")).chars().count();
        match pending {
            Pending::Skill(name) => add_skill(audit, &name, chars),
            Pending::Reference(path) => {
                *audit.chars.entry(Kind::SkillReference).or_default() += chars;
                *audit.references.entry(path).or_default() += chars;
            }
        }
    }
}

fn assistant_record(state: &mut State, message: &Value) {
    let Some(Value::Array(items)) = message.get("content") else { return };
    for item in items.iter().filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_use")) {
        let id = item.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        let input = item.get("input");
        let field = |key: &str| input.and_then(|input| input.get(key)).and_then(Value::as_str);
        match item.get("name").and_then(Value::as_str) {
            Some("Skill") => {
                if let Some(skill) = field("skill") {
                    state.pending.insert(id, Pending::Skill(skill.rsplit(':').next().unwrap_or(skill).to_owned()));
                }
            }
            Some("Read") => {
                let Some(path) = field("file_path") else { continue };
                state.accesses.push((state.step, path_key(path)));
                if let Some(found) = SKILL_FILE.captures(path) {
                    state.pending.insert(id, Pending::Skill(found[1].to_owned()));
                } else if let Some(found) = REFERENCE_FILE.captures(path) {
                    state.pending.insert(id, Pending::Reference(found[1].to_owned()));
                }
            }
            Some("Grep") => {
                if let Some(path) = field("path") {
                    state.accesses.push((state.step, path_key(path)));
                }
            }
            Some("Bash") => {
                let Some(command) = field("command").filter(|command| READ_VERB.is_match(command)) else { continue };
                for token in command.split(|c: char| c.is_whitespace() || "'\"`;|&()<>=".contains(c)).filter(|token| token.contains('/')) {
                    state.accesses.push((state.step, path_key(token)));
                }
            }
            _ => {}
        }
    }
}

pub fn path_key(path: &str) -> String {
    let path = path.trim_end_matches([':', ',', '.', ')']);
    if let Some(found) = REPO_PREFIX.find(path) {
        return path[found.end()..].to_owned();
    }
    match HOME_PREFIX.find(path) {
        Some(found) => format!("~/{}", &path[found.end()..]),
        None => path.strip_prefix("./").unwrap_or(path).to_owned(),
    }
}

pub fn named_paths(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    NAMED_PATH
        .captures_iter(text)
        .map(|found| path_key(&found[1]))
        .filter(|key| key.rsplit('/').next().is_some_and(|last| last.contains('.')))
        .filter(|key| seen.insert(key.clone()))
        .collect()
}

fn same_file(named: &str, accessed: &str) -> bool {
    named == accessed || accessed.ends_with(&format!("/{named}")) || named.ends_with(&format!("/{accessed}"))
}

fn settle_precision(audit: &mut Audit, state: &State) {
    for (step, paths) in &state.injections {
        audit.grounding.named += paths.len();
        audit.grounding.read += paths
            .iter()
            .filter(|named| state.accesses.iter().any(|(at, accessed)| at > step && same_file(named, accessed)))
            .count();
    }
}
