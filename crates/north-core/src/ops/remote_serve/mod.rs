pub mod south_policy;
mod env;
mod lexer;
mod verbs;

use std::collections::BTreeMap;
use std::fmt;

pub const MAX_COMMAND_BYTES: usize = 80 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub verb: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Empty,
    TooLong,
    UnterminatedQuote,
    Metacharacter(char),
    EnvKey(String),
    EnvValue(String),
    EnvRepeated(String),
    Verb(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(out, "empty command"),
            Self::TooLong => write!(out, "command longer than {MAX_COMMAND_BYTES} bytes"),
            Self::UnterminatedQuote => write!(out, "unterminated quote"),
            Self::Metacharacter(c) => write!(out, "shell metacharacter {c:?}"),
            Self::EnvKey(key) => write!(out, "environment key {key} is not allowed"),
            Self::EnvValue(key) => write!(out, "environment value for {key} does not match its pattern"),
            Self::EnvRepeated(key) => write!(out, "environment key {key} given twice"),
            Self::Verb(verb) => write!(out, "command {verb} is not on the allowlist"),
        }
    }
}

pub fn plan(original_command: &str) -> Result<Plan, Refusal> {
    if original_command.len() > MAX_COMMAND_BYTES {
        return Err(Refusal::TooLong);
    }
    let words = lexer::split(original_command)?;
    let (env, argv) = env::split_prefix(&words)?;
    if argv.is_empty() {
        return Err(Refusal::Empty);
    }
    let verb = verbs::authorize(argv)?;
    Ok(Plan { verb, argv: argv.to_vec(), env })
}

#[cfg(test)]
mod tests;
