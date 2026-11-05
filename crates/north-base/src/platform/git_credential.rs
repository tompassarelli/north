use std::io::{BufRead, Write};

use super::github_app::{self, AGENT_APP, Host, Request};
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north git-credential get|store|erase

The git credential helper agent sessions push to GitHub through: for https://github.com it answers
get with an installation token of the Socrates Agent App (north gh-app-token), so GitHub's tag
rulesets, which only the admin role and that App's v* release path bypass, bind agent pushes.
Other hosts, store and erase get no answer. Fleet sessions set it through GIT_CONFIG_COUNT, so
Tom's own git and gh login stay unchanged.";

const SCOPE: &str = "credential.https://github.com.helper";
const HELPER: &str = "!north git-credential";

pub fn session_env() -> Vec<(String, String)> {
    let entries = [(SCOPE, ""), (SCOPE, HELPER)];
    let mut env = vec![("GIT_CONFIG_COUNT".to_owned(), entries.len().to_string())];
    for (index, (key, value)) in entries.into_iter().enumerate() {
        env.push((format!("GIT_CONFIG_KEY_{index}"), key.to_owned()));
        env.push((format!("GIT_CONFIG_VALUE_{index}"), value.to_owned()));
    }
    env
}

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [word] if word == "-h" || word == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let mint = || {
        let request = Request::installation(AGENT_APP).ok_or_else(|| format!("unknown app {AGENT_APP}"))?;
        github_app::mint(&request, &Host::from_env(), Timestamp::now().unix_seconds()).or_else(|why| session_token(std::env::var("GH_TOKEN").ok()).ok_or(why))
    };
    match answer(arguments, std::io::stdin().lock(), mint) {
        Ok(Some(reply)) => match std::io::stdout().write_all(reply.as_bytes()) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Ok(None) => 0,
        Err(message) => {
            eprintln!("north git-credential: {message}");
            1
        }
    }
}

/// The installation token the launcher gave this session, for a host that holds no sops key to mint a fresh one from.
fn session_token(token: Option<String>) -> Option<String> {
    token.filter(|token| token.starts_with("ghs_"))
}

fn answer(arguments: &[String], input: impl BufRead, mint: impl FnOnce() -> Result<String, String>) -> Result<Option<String>, String> {
    match arguments {
        [action] if action == "get" => {}
        [action] if action == "store" || action == "erase" => return Ok(None),
        _ => return Err(USAGE.to_owned()),
    }
    let mut protocol = None;
    let mut host = None;
    for line in input.lines() {
        let line = line.map_err(|error| format!("cannot read the request: {error}"))?;
        if line.is_empty() {
            break;
        }
        match line.split_once('=') {
            Some(("protocol", value)) => protocol = Some(value.to_owned()),
            Some(("host", value)) => host = Some(value.to_owned()),
            _ => {}
        }
    }
    if protocol.as_deref() != Some("https") || host.as_deref() != Some("github.com") {
        return Ok(None);
    }
    let token = mint()?;
    Ok(Some(format!("username=x-access-token\npassword={token}\n")))
}

#[cfg(test)]
#[path = "git_credential_tests.rs"]
mod tests;
