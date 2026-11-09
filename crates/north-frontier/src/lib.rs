//! The frontier benchmark: north against plain Claude Code on the same landed boxes (docs/rationale/frontier.md, north#730).
//! `north test frontier` replays each box from its base under both arms in a sandbox, grades each arm's tree by the
//! landing's own tests and writes one run row per arm and task; `north test frontier ab-report` writes the live A/B's result into frontier.md once each arm has landed 30 boxes: assignment-to-landed time and the short- and long-term usage per landed box; `north dash --frontier` prints the rows'
//! metrics and intervals. Nothing here reads or writes live fleet, ledger or landing state while an arm runs; once an arm ends, the tokens its pool key spent are written to the ledger as runs of that key's org.
use std::path::PathBuf;

use north_base::error::NorthError;

pub mod ab;
mod assign;
mod billing;
mod cli;
mod config;
mod followup;
mod grade;
mod live;
mod oracle;
mod process;
pub mod report;
pub mod rows;
mod sandbox;
mod session;
pub mod stats;
mod t3code;
mod task;

#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north test frontier tasks --since DATE --until DATE --repo DIR --briefs DIR | run TASKS.json --briefs DIR --out DIR [--arm north|plain|t3code]... [--t3 DIR --t3-node PATH] [--plan ACCOUNT] [--only ID]... [--jobs N] [--budget-usd X] [--stop-usd X] [--wall-min M] [--idle-min M] | box ISSUE --box TEXT --tree DIR --base DIR --t3 DIR --t3-node PATH --account LABEL [--owner DIR] [--wall-min M] | grade TASKS.json --out DIR [--only ID]... | report TASKS.json --out DIR [--rows FILE] | ab-report [--rows FILE] [--doc FILE] [--ledger FILE] [--ref REV] | ab-assign [--root DIR] [--rows FILE] [--repo OWNER/REPO]... [--seed N] [--count N] | ab-record BOX --sha SHA --at UNIX_SECONDS [--tokens-in N] [--tokens-out N] [--cost-usd X] [--model ID] [--effort LEVEL] [--root DIR] [--rows FILE]";

pub fn run(arguments: &[String]) -> u8 {
    match cli::dispatch(arguments) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(NorthError::Usage(message)) if message.is_empty() => {
            eprintln!("{USAGE}");
            64
        }
        Err(error) => {
            eprintln!("north test frontier: {error}");
            1
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn refusal(message: impl Into<String>) -> NorthError {
    NorthError::Usage(message.into())
}
