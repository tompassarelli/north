use std::time::Instant;

use serde_json::{Value, json};

use crate::proc::run;
use crate::task::Tree;

pub const SYSTEM: &str = "You predict which files a coding task will change in a repository. You see only the task text and a list of the repository's files. Answer with at most 10 repository paths, one per line, the most likely first, copied exactly from the list. Write nothing else.";
pub const PROPOSE_LIMIT: usize = 10;

pub fn whole_map_input(text: &str, map: &str) -> String {
    format!("Task:\n{text}\n\nThe repository's files at the task's base commit, each with the names it declares:\n{map}\n")
}

pub fn candidates_input(text: &str, candidates: &str) -> String {
    format!("Task:\n{text}\n\nCandidate files found by searching the task's words and by the knowledge graph, each with the names it declares. Choose only from these:\n{candidates}\n")
}

const ATTEMPTS: usize = 8;
const PAUSE: std::time::Duration = std::time::Duration::from_secs(3);

fn once(north: &str, model: &str, input: &str) -> Result<Value, String> {
    let args = ["check", "oneshot", "--model", model, "--effort", "low", "--system", SYSTEM, "--json"];
    let started = Instant::now();
    let ran = run(north, &args, &[], &[], Some(input.as_bytes().to_vec()));
    if ran.code != 0 {
        return Err(format!("{north} check oneshot: exit {}: {}", ran.code, ran.err.trim()));
    }
    let reply: Value = serde_json::from_slice(&ran.out).map_err(|e| format!("{north} check oneshot printed no JSON: {e}"))?;
    let tokens = &reply["tokens"];
    let counted = |key: &str| tokens[key].as_u64().unwrap_or(0);
    Ok(json!({
        "answer": reply["text"],
        "wall_ms": started.elapsed().as_millis() as u64,
        "input_tokens": counted("input") + counted("cache_read") + counted("cache_write"),
        "output_tokens": counted("output"),
    }))
}

pub fn ask(north: &str, model: &str, input: &str) -> Result<Value, String> {
    let mut last = String::new();
    for _ in 0..ATTEMPTS {
        match once(north, model, input) {
            Ok(reply) => return Ok(reply),
            Err(why) => last = why,
        }
        std::thread::sleep(PAUSE);
    }
    Err(last)
}

pub struct Proposal {
    pub files: Vec<String>,
    pub invented: usize,
}

pub fn parse(answer: &str, tree: &Tree) -> Proposal {
    let (mut files, mut invented) = (Vec::new(), 0);
    for line in answer.lines() {
        let path = line.trim().trim_start_matches(|c: char| c.is_ascii_digit() || ".-*) ".contains(c)).trim_matches('`').split(':').next().unwrap_or("").trim();
        if path.is_empty() {
            continue;
        }
        if !tree.has(path) {
            invented += 1;
        } else if !files.iter().any(|seen| seen == path) && files.len() < PROPOSE_LIMIT {
            files.push(path.to_owned());
        }
    }
    Proposal { files, invented }
}
