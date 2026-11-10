use std::io::{BufRead, Write};

use super::github_app;
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north git-credential get|store|erase

The git credential helper agent sessions push to GitHub through: for https://github.com it answers
get with an installation token of the North Fleet App from the credential broker (north broker
github-token), limited to the repository the push names, so GitHub's tag
rulesets, which only the admin role and that App's v* release path bypass, bind agent pushes.
A host the laptop pushes that token to (gex45 and ccx33 hold no App key) answers with the pushed
token while it is fresh; a host that cannot mint answers with the same App's token the session
was launched with in GH_TOKEN and names it on stderr. Other hosts, store and erase get no
answer. Fleet sessions set it through GIT_CONFIG_COUNT, so Tom's own git and gh login stay unchanged.";

const SCOPE: &str = "credential.https://github.com.helper";
const HELPER: &str = "!north git-credential";
const PATH_SCOPE: &str = "credential.https://github.com.useHttpPath";

pub fn session_env() -> Vec<(String, String)> {
    session_env_with(&[])
}

/// `session_env` with the `extra` git configuration after the helper's; GIT_CONFIG_COUNT entries outrank a repository's own configuration.
pub fn session_env_with(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let entries: Vec<(&str, &str)> = [(SCOPE, ""), (SCOPE, HELPER), (PATH_SCOPE, "true")].into_iter().chain(extra.iter().copied()).collect();
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
    let token = |path: &str| {
        let (token, note) = github_app::agent_token(Timestamp::now().unix_seconds(), repository(path), std::env::var("GH_TOKEN").ok())?;
        if let Some(note) = note {
            eprintln!("north git-credential: {note}");
        }
        Ok(token)
    };
    match answer(arguments, std::io::stdin().lock(), token) {
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

fn repository(path: &str) -> Option<&str> {
    let (owner, name) = path.trim_matches('/').split_once('/')?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then_some(name)
}

fn answer(arguments: &[String], input: impl BufRead, token: impl FnOnce(&str) -> Result<String, String>) -> Result<Option<String>, String> {
    match arguments {
        [action] if action == "get" => {}
        [action] if action == "store" || action == "erase" => return Ok(None),
        _ => return Err(USAGE.to_owned()),
    }
    let mut protocol = None;
    let mut host = None;
    let mut path = String::new();
    for line in input.lines() {
        let line = line.map_err(|error| format!("cannot read the request: {error}"))?;
        if line.is_empty() {
            break;
        }
        match line.split_once('=') {
            Some(("protocol", value)) => protocol = Some(value.to_owned()),
            Some(("host", value)) => host = Some(value.to_owned()),
            Some(("path", value)) => value.clone_into(&mut path),
            _ => {}
        }
    }
    if protocol.as_deref() != Some("https") || host.as_deref() != Some("github.com") {
        return Ok(None);
    }
    let token = token(&path)?;
    Ok(Some(format!("username=x-access-token\npassword={token}\n")))
}

#[cfg(test)]
#[path = "git_credential_tests.rs"]
mod tests;
