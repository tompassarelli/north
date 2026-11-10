//! The probe that judges whether an account's client serves it: one one-turn Haiku prompt through the same
//! client, credential and config the fleet launches with.
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::fleet::install;
use crate::fleet::model::Account;
use crate::platform::process::run_command_within;

const LIMIT: Duration = Duration::from_secs(120);
const PROMPT: &str = "Reply with the single word ok.";

/// What the client answered on an account.
#[derive(Debug, Eq, PartialEq)]
pub enum Answer {
    Passed,
    /// The client refused the account itself (subscription access disabled, signed out, a revoked login), with its text.
    Refused(String),
    /// Anything else that kept the probe from passing, which says nothing about the account.
    Failed(String),
}

/// Asks the client on `account` for one short answer.
pub fn probe(account: &Account) -> Answer {
    let invocation = super::invocation(account);
    let claude = install::runtime("claude").display().to_string();
    let args = [claude.as_str(), "-p", PROMPT, "--output-format", "json", "--model", "haiku", "--max-turns", "1", "--strict-mcp-config"];
    let mut argv = invocation.prefix.iter().map(String::as_str).chain(args);
    let mut command = Command::new(argv.next().unwrap_or_default());
    command.args(argv).env("DISABLE_AUTOUPDATER", "1").env("DISABLE_INSTALLATION_CHECKS", "1").envs(invocation.env);
    for name in &invocation.env_remove {
        command.env_remove(name);
    }
    match run_command_within(command, None, LIMIT) {
        Some(output) => answer(&String::from_utf8_lossy(&output.stdout), &String::from_utf8_lossy(&output.stderr)),
        None => Answer::Failed(format!("the client did not answer within {} s", LIMIT.as_secs())),
    }
}

fn answer(stdout: &str, stderr: &str) -> Answer {
    let result = stdout.lines().rev().find_map(|line| serde_json::from_str::<Value>(line.trim()).ok().filter(|frame| frame["type"] == "result"));
    match result {
        Some(result) if result["is_error"] == false => Answer::Passed,
        Some(result) => match super::turns::credential_failure(&result) {
            Some(refusal) => Answer::Refused(refusal.text),
            None => Answer::Failed(result["result"].as_str().unwrap_or("the client answered with an error").to_owned()),
        },
        None => Answer::Failed(stdout.lines().chain(stderr.lines()).map(str::trim).find(|line| !line.is_empty()).unwrap_or("the client printed no result").to_owned()),
    }
}
