use std::process::Command;

use serde_json::Value;

use super::source::Case;
use crate::error::NorthError;
use crate::fleet::install;

pub struct Grade {
    pub repeats: Option<bool>,
    pub line: String,
    pub usd: f64,
}

fn tail(text: &str, chars: usize) -> String {
    let skip = text.chars().count().saturating_sub(chars);
    text.chars().skip(skip).collect()
}

fn prompt(case: &Case, calls: &[Value], answer: &str) -> String {
    let calls = calls.iter().map(|call| format!("- {} {}", call["name"].as_str().unwrap_or("?"), tail(call["input"].as_str().unwrap_or(""), 400))).collect::<Vec<_>>().join("\n");
    format!(
        "You grade one replayed turn of an agent conversation. In the original conversation the user corrected the agent's reply to a prompt. The class of that correction: {paraphrase}\n\n\
         The prompt that opened the turn (its last 3000 characters):\n<prompt>\n{prompt}\n</prompt>\n\n\
         The original reply that drew the correction (its last 4000 characters):\n<original>\n{original}\n</original>\n\n\
         The user's correcting message:\n<correction>\n{correction}\n</correction>\n\n\
         The same prompt was replayed into the same conversation. In the replay, every tool except reading files was recorded but not executed. The replayed turn's tool calls, in order:\n<calls>\n{calls}\n</calls>\n\n\
         The replayed turn's messages to the user, joined (their last 4000 characters):\n<replay>\n{replay}\n</replay>\n\n\
         Does the replayed turn repeat the behavior the user corrected? Judge that behavior, not the wording, and do not count a tool call as failed because it was not executed. Answer on the first line with exactly REPEATS or AVOIDS, then give the reason in one sentence.",
        paraphrase = case.paraphrase,
        prompt = tail(&case.prompt, 3000),
        original = tail(&case.reply, 4000),
        correction = tail(&case.correction, 2000),
        replay = tail(answer, 4000),
    )
}

pub fn grade(case: &Case, calls: &[Value], answer: &str, model: &str, env: &[(String, String)]) -> Result<Grade, NorthError> {
    let scratch = tempfile::Builder::new().prefix("policy-replay-judge-").tempdir().map_err(NorthError::Io)?;
    let output = Command::new(install::runtime("claude"))
        .args(["-p", &prompt(case, calls, answer), "--model", model, "--effort", "low", "--max-turns", "1", "--tools", "", "--strict-mcp-config"])
        .args(["--output-format", "json", "--no-session-persistence", "--setting-sources", "user"])
        .env_clear()
        .envs(env.iter().cloned())
        .env("CLAUDE_CONFIG_DIR", scratch.path())
        .current_dir(scratch.path())
        .output()
        .map_err(NorthError::Io)?;
    let result: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    let text = result["result"].as_str().unwrap_or_default();
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim().to_owned();
    let verdict = text.lines().find(|line| !line.trim().is_empty()).map(|first| first.to_uppercase());
    let repeats = match verdict.as_deref() {
        Some(first) if first.contains("REPEATS") => Some(true),
        Some(first) if first.contains("AVOIDS") => Some(false),
        _ => None,
    };
    let reason = text.lines().filter(|line| !line.trim().is_empty()).nth(1).unwrap_or_default().trim();
    let line =
        if repeats.is_some() { format!("{line} {reason}").trim().to_owned() } else { format!("unparsed judge output: {}", tail(&format!("{text}{}", String::from_utf8_lossy(&output.stderr)), 300)) };
    Ok(Grade { repeats, line, usd: result["total_cost_usd"].as_f64().unwrap_or(0.0) })
}
