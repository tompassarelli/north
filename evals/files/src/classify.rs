use std::time::Instant;

use serde_json::{Value, json};

use crate::proc::run;
use crate::task::Tree;

pub const SYSTEM: &str = "You predict which files a coding task will change in a repository. You see only the task text and a list of the repository's files. Answer with at most 10 repository paths, one per line, the most likely first, copied exactly from the list. Write nothing else.";
const EXTERNAL_BILLING: [&str; 6] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY"];
pub const PROPOSE_LIMIT: usize = 10;

pub fn whole_map_input(text: &str, map: &str) -> String {
    format!("Task:\n{text}\n\nThe repository's files at the task's base commit, each with the names it declares:\n{map}\n")
}

pub fn candidates_input(text: &str, candidates: &str) -> String {
    format!("Task:\n{text}\n\nCandidate files found by searching the task's words and by the knowledge graph, each with the names it declares. Choose only from these:\n{candidates}\n")
}

pub struct Account {
    pub label: String,
    pub token: String,
    pub profile: String,
}

pub fn accounts(labels: &str, home: &str) -> Result<Vec<Account>, String> {
    labels.split(',').map(|label| {
        let token = std::fs::read_to_string(format!("{home}/.local/state/north/claude-tokens/claude-{label}")).map_err(|e| format!("account {label}: {e}"))?;
        Ok(Account { label: label.to_owned(), token: token.trim().to_owned(), profile: format!("{home}/.local/state/north/profiles/claude-{label}") })
    }).collect()
}

fn once(account: &Account, model: &str, input: &str) -> Result<Value, String> {
    let env = [("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"), ("CLAUDE_CODE_DISABLE_FAST_MODE", "1"), ("CLAUDE_CODE_OAUTH_TOKEN", account.token.as_str()), ("CLAUDE_CONFIG_DIR", account.profile.as_str())];
    let args = ["-p", "--model", model, "--effort", "low", "--system-prompt", SYSTEM, "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--disable-slash-commands", "--no-session-persistence", "--output-format", "json"];
    let started = Instant::now();
    let ran = run("claude", &args, &env, &EXTERNAL_BILLING, Some(input.as_bytes().to_vec()));
    let reply: Value = serde_json::from_slice(&ran.out).map_err(|e| format!("claude on {}: {e}: {}", account.label, ran.err.trim()))?;
    if reply["is_error"] == true {
        return Err(format!("claude on {}: {}", account.label, reply["result"].as_str().unwrap_or("error")));
    }
    let usage = &reply["usage"];
    let tokens = |key: &str| usage[key].as_u64().unwrap_or(0);
    Ok(json!({
        "answer": reply["result"],
        "account": account.label,
        "wall_ms": started.elapsed().as_millis() as u64,
        "api_ms": reply["duration_api_ms"],
        "input_tokens": tokens("input_tokens") + tokens("cache_creation_input_tokens") + tokens("cache_read_input_tokens"),
        "output_tokens": tokens("output_tokens"),
        "cost_usd": reply["total_cost_usd"],
    }))
}

pub fn ask(accounts: &[Account], first: usize, model: &str, input: &str) -> Result<Value, String> {
    let mut failures = Vec::new();
    for turn in 0..accounts.len() * 2 {
        match once(&accounts[(first + turn) % accounts.len()], model, input) {
            Ok(reply) => return Ok(reply),
            Err(why) => failures.push(why),
        }
    }
    Err(failures.join("; "))
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
