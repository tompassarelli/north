use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::NorthError;
use crate::threads::incidents::corrections;

pub struct Case {
    pub id: String,
    pub class: String,
    pub paraphrase: String,
    pub session: String,
    pub project: String,
    pub cwd: PathBuf,
    pub chain: Vec<Value>,
    pub prompt: String,
    pub reply: String,
    pub correction: String,
    pub model: String,
    pub effort: String,
    pub cut_at: String,
}

fn refusal(message: String) -> NorthError {
    NorthError::Usage(format!("policy-replay case: {message}"))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn ledger() -> PathBuf {
    std::env::var_os("NORTH_GROUNDING_LEDGER").map_or_else(|| home().join(".local/state/north/grounding/ledger.jsonl"), PathBuf::from)
}

fn correction_row(id: &str) -> Result<Value, NorthError> {
    let text = fs::read_to_string(ledger()).map_err(NorthError::Io)?;
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|row| row["kind"] == "correction" && row["id"] == id)
        .ok_or_else(|| refusal(format!("no correction row {id} in {}", ledger().display())))
}

fn project_roots() -> Vec<PathBuf> {
    let mut roots = vec![home().join(".claude/projects")];
    if let Ok(profiles) = fs::read_dir(home().join(".local/state/north/profiles")) {
        roots.extend(profiles.flatten().map(|profile| profile.path().join("projects")));
    }
    roots
}

fn transcripts(session: &str) -> Vec<PathBuf> {
    let file = format!("{session}.jsonl");
    project_roots().iter().filter_map(|root| fs::read_dir(root).ok()).flat_map(|projects| projects.flatten().map(|project| project.path().join(&file))).filter(|path| path.is_file()).collect()
}

fn text_of(record: &Value) -> Option<String> {
    let content = &record["message"]["content"];
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    let blocks = content.as_array()?;
    if blocks.iter().any(|block| block["type"] == "tool_result") {
        return None;
    }
    let text = blocks.iter().filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n");
    (!text.is_empty()).then_some(text)
}

fn is_prompt(record: &Value) -> bool {
    record["type"] == "user" && record["isMeta"] != true && record["isCompactSummary"] != true && text_of(record).is_some()
}

fn queued(record: &Value) -> Option<String> {
    let attachment = &record["attachment"];
    (attachment["type"] == "queued_command" && attachment["commandMode"] == "prompt").then(|| attachment["prompt"].as_str().map(str::to_owned))?
}

fn typed(record: &Value) -> Option<String> {
    if is_prompt(record) { text_of(record) } else { queued(record) }
}

fn assistant_text(record: &Value) -> String {
    record["message"]["content"].as_array().into_iter().flatten().filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n")
}

struct Transcript {
    lines: Vec<Value>,
    by_uuid: HashMap<String, usize>,
}

impl Transcript {
    fn read(path: &Path) -> Result<Self, NorthError> {
        let text = fs::read_to_string(path).map_err(NorthError::Io)?;
        let lines: Vec<Value> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
        let by_uuid = lines.iter().enumerate().filter_map(|(index, record)| Some((record["uuid"].as_str()?.to_owned(), index))).collect();
        Ok(Self { lines, by_uuid })
    }

    fn parent(&self, index: usize) -> Option<usize> {
        self.lines[index]["parentUuid"].as_str().and_then(|uuid| self.by_uuid.get(uuid).copied())
    }

    fn correction_at(&self, at: &str) -> Option<usize> {
        let second = at.get(..19)?;
        self.lines.iter().position(|record| typed(record).is_some() && record["timestamp"].as_str().is_some_and(|stamp| stamp.starts_with(second)))
    }
}

fn effort_before(lines: &[Value], index: usize) -> Option<String> {
    lines[..index].iter().rev().filter(|record| record["type"] == "api-request-blob").find_map(|record| record["message"]["output_config"]["effort"].as_str().map(str::to_owned))
}

pub fn resolve(id: &str) -> Result<Case, NorthError> {
    let row = correction_row(id)?;
    let at = row["at"].as_str().ok_or_else(|| refusal(format!("{id} has no at")))?;
    let source = row["source"].as_str().unwrap_or_default();
    let session = source.split(' ').find_map(|word| word.strip_prefix("session:")).ok_or_else(|| refusal(format!("{id} names no session")))?;
    if !source.starts_with("tom") {
        return Err(refusal(format!("{id} was recorded by {}, not typed into the session, so no turn of it was corrected", source.split(' ').next().unwrap_or("?"))));
    }
    let class = row["class"].as_str().unwrap_or(corrections::UNCLASSIFIED).to_owned();
    for path in transcripts(session) {
        let transcript = Transcript::read(&path)?;
        let Some(corrected) = transcript.correction_at(at) else {
            continue;
        };
        let mut turn = Vec::new();
        let mut cursor = transcript.parent(corrected);
        let prompt = loop {
            let Some(index) = cursor else { break None };
            if is_prompt(&transcript.lines[index]) {
                break Some(index);
            }
            turn.push(index);
            cursor = transcript.parent(index);
        };
        let Some(prompt) = prompt else { continue };
        let mut chain = Vec::new();
        let mut cursor = transcript.parent(prompt);
        while let Some(index) = cursor {
            chain.push(transcript.lines[index].clone());
            cursor = transcript.parent(index);
        }
        chain.reverse();
        let chain = without_goal(chain);
        let reply = turn
            .iter()
            .rev()
            .map(|&index| &transcript.lines[index])
            .filter(|record| record["type"] == "assistant")
            .map(assistant_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let model = turn.iter().map(|&index| &transcript.lines[index]).find_map(|record| record["message"]["model"].as_str()).unwrap_or("opus").to_owned();
        let prompt_record = &transcript.lines[prompt];
        return Ok(Case {
            id: id.to_owned(),
            paraphrase: corrections::paraphrase(&class),
            class,
            session: session.to_owned(),
            project: path.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            cwd: PathBuf::from(prompt_record["cwd"].as_str().unwrap_or("/")),
            chain,
            prompt: text_of(prompt_record).unwrap_or_default(),
            reply,
            correction: typed(&transcript.lines[corrected]).unwrap_or_default(),
            model,
            effort: effort_before(&transcript.lines, prompt).unwrap_or_else(|| "high".to_owned()),
            cut_at: prompt_record["timestamp"].as_str().unwrap_or(at).to_owned(),
        });
    }
    Err(refusal(format!("no transcript of session {session} holds the correction of {id} at {at} with a prompt before it")))
}

fn without_goal(chain: Vec<Value>) -> Vec<Value> {
    let mut parent = Value::Null;
    chain
        .into_iter()
        .filter(|record| record["attachment"]["type"] != "goal_status")
        .map(|mut record| {
            if !parent.is_null() {
                record["parentUuid"] = parent.clone();
            }
            parent = record["uuid"].clone();
            record
        })
        .collect()
}
