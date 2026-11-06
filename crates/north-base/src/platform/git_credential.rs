use std::io::{BufRead, Write};

use super::github_app::{self, AGENT_APP, Host, Request};
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north git-credential get|store|erase

The git credential helper agent sessions push to GitHub through: for https://github.com it answers
get with an installation token of the Socrates Agent App (north gh-app-token), so GitHub's tag
rulesets, which only the admin role and that App's v* release path bypass, bind agent pushes.
A host the laptop pushes that token to (gex45 and ccx33 hold no App key) answers with the pushed
token while it is fresh; a host that cannot mint answers with the same App's token the session
was launched with in GH_TOKEN and names it on stderr. Other hosts, store and erase get no
answer. Fleet sessions set it through GIT_CONFIG_COUNT, so Tom's own git and gh login stay unchanged.";

const SCOPE: &str = "credential.https://github.com.helper";
const HELPER: &str = "!north git-credential";
const PATH_SCOPE: &str = "credential.https://github.com.useHttpPath";

pub fn session_env() -> Vec<(String, String)> {
    let entries = [(SCOPE, ""), (SCOPE, HELPER), (PATH_SCOPE, "true")];
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
    let token = |_: &str| {
        let request = Request::installation(AGENT_APP).ok_or_else(|| format!("unknown app {AGENT_APP}"))?;
        github_app::mint(&request, &Host::from_env(), Timestamp::now().unix_seconds()).or_else(|why| {
            let Some((source, token)) = fallback_token() else { return Err(why) };
            eprintln!("north git-credential: cannot mint the Socrates Agent App token ({why}); answering with {source}");
            Ok(token)
        })
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

/// What a host that cannot mint answers with: the installation token the launcher gave the session.
pub(super) fn fallback_token() -> Option<(&'static str, String)> {
    session_token(std::env::var("GH_TOKEN").ok()).map(|token| ("the session's GH_TOKEN", token))
}

fn session_token(token: Option<String>) -> Option<String> {
    token.filter(|token| token.starts_with("ghs_"))
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
