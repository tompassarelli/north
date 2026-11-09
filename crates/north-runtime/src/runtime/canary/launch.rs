use super::super::failure::{DEFERRED, Failure, failed};
use crate::platform::process;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const PROMPT: &str = "Reply with the single word ok and nothing else.";
const ASK: Duration = Duration::from_secs(30);
const ANSWER_WITHIN: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(3);
const PLACEMENT_BRIEF: &str = "Surface: shell\nNeeds: laptop\n";

pub(in crate::runtime) fn probe(provider: &str) -> Result<(), Failure> {
    let north = std::env::current_exe().map_err(|error| unrun(error.into()))?;
    let scratch = tempfile::tempdir().map_err(|error| unrun(error.into()))?;
    let state = scratch.path().join("state");
    let cwd = scratch.path().join("work");
    std::fs::create_dir_all(&state).and_then(|()| std::fs::create_dir_all(&cwd)).map_err(|error| unrun(error.into()))?;
    let name = format!("canary-{provider}-{}", std::process::id());
    let answered = match launch(&north, &state, provider, &name, &cwd) {
        Ok(true) => answer(&north, &state, &name),
        Ok(false) => Ok(()),
        Err(failure) => Err(unrun(failure)),
    };
    let _ = fleet(&north, &state, &["forget", &name, "--force", "runtime canary probe finished"]);
    answered
}

fn unrun(failure: Failure) -> Failure {
    failed(DEFERRED, format!("the launch never reached a worker, so the probe has no verdict: {}", failure.message))
}

fn fleet(north: &Path, state: &Path, words: &[&str]) -> Result<String, String> {
    let mut command = Command::new(north);
    command.arg("fleet").args(words);
    command.env("XDG_STATE_HOME", state).env("NORTH_TEST_ROOT", state).env("THREADS_DB", state.join("threads.db"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("NORTH_FLEET_")) {
        command.env_remove(key);
    }
    let output = process::try_run_command_within(command, None, ASK).map_err(|error| error.to_string())?.ok_or_else(|| format!("north fleet {} gave no answer within {}s", words[0], ASK.as_secs()))?;
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        false => Err(format!("north fleet {} failed: {}", words[0], String::from_utf8_lossy(&output.stderr).trim())),
    }
}

fn launch(north: &Path, state: &Path, provider: &str, name: &str, cwd: &Path) -> Result<bool, Failure> {
    let cwd = cwd.to_string_lossy();
    let mut words = vec!["launch", "--provider", provider, "--name", name, "--parent", "none", "--test", "--no-wait", "--cwd", &cwd];
    let account;
    if provider == "claude" {
        words.extend(["--model", "haiku"]);
        match placed_claude_account(north)? {
            Some(placed) => {
                account = placed;
                words.extend(["--account", &account]);
            }
            None => {
                println!("north runtime canary probe: no Claude account has headroom here, so the launch is not judged");
                return Ok(false);
            }
        }
    }
    words.push(PROMPT);
    fleet(north, state, &words).map(|_| true).map_err(|why| failed(1, why))
}

fn placed_claude_account(north: &Path) -> Result<Option<String>, Failure> {
    let brief = tempfile::Builder::new().suffix(".md").tempfile()?;
    std::fs::write(brief.path(), PLACEMENT_BRIEF)?;
    let mut command = Command::new(north);
    command.args(["config", "agents", "plan", "--place"]).arg(brief.path());
    let output = process::try_run_command_within(command, None, ASK)?.ok_or_else(|| failed(1, "placing a Claude account gave no answer"))?;
    Ok(placed_account(&String::from_utf8_lossy(&output.stdout)))
}

fn placed_account(listing: &str) -> Option<String> {
    let target = listing.lines().find_map(|line| line.split_once(": target ").map(|(_, rest)| rest))?;
    let mut words = target.split_whitespace();
    let host = words.next()?;
    words.next().and_then(|word| word.strip_prefix("claude:")).filter(|_| !host.ends_with(':')).map(str::to_owned)
}

fn answer(north: &Path, state: &Path, name: &str) -> Result<(), Failure> {
    let started = Instant::now();
    let mut last = "no transcript yet".to_owned();
    while started.elapsed() < ANSWER_WITHIN {
        match fleet(north, state, &["show", name, "--json"]) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(session) => match answered(&session) {
                    Some(reply) if !reply.trim().is_empty() => return Ok(()),
                    _ => last = format!("status {}", session["status"].as_str().unwrap_or("unknown")),
                },
                Err(error) => last = format!("fleet show printed invalid JSON: {error}"),
            },
            Err(why) => last = why,
        }
        std::thread::sleep(POLL);
    }
    Err(failed(1, format!("{name} gave no answer within {}s: {last}", ANSWER_WITHIN.as_secs())))
}

fn answered(session: &Value) -> Option<String> {
    let entries = session["entries"].as_array()?;
    let last = entries.iter().rev().find(|entry| matches!(entry["kind"].as_str(), Some("user" | "assistant")))?;
    (last["kind"] == "assistant" && session["status"] != "working").then(|| last["text"].as_str().unwrap_or_default().to_owned())
}
