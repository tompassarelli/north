use std::io::{BufRead, Write};

use super::github_app;
use north_foundation::foundation::clock::Timestamp;

const USAGE: &str = "usage: north git-credential get|store|erase

The git credential helper agent sessions push to GitHub through: for https://github.com it answers
get with an installation token of the North Fleet App from the credential broker (north broker
github-token), limited to the repository the push names, so GitHub's tag
rulesets, which only the admin role and that App's v* release path bypass, bind agent pushes.
A host the laptop pushes that token to (gex45 and ccx33 hold no App key) answers with the pushed
token while it is fresh; a host that cannot mint answers with the same App's token the session
was launched with in GH_TOKEN and names it on stderr. Other hosts, store and erase get no
answer. Fleet sessions set it through GIT_CONFIG_COUNT, so Tom's own git and gh login stay unchanged. A confined worker command's git asks `north git-credential confined`, which reads no credential in its domain, and a lane-only session's brokered lane push asks `north git-credential lane OWNER/REPO`, which answers only for its own repository (north#1397).";

const SCOPE: &str = "credential.https://github.com.helper";
pub const HELPER: &str = "!north git-credential";
pub const CONFINED: &str = "confined";
pub const LANE: &str = "lane";
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
        Ok(Some(reply)) => u8::from(std::io::stdout().write_all(reply.as_bytes()).is_err()),
        Ok(None) => 0,
        Err(message) => {
            eprintln!("north git-credential: {message}");
            1
        }
    }
}

pub fn repository(path: &str) -> Option<&str> {
    let (owner, name) = path.trim_matches('/').trim_end_matches(".git").split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then_some(name)
}

pub fn answer(arguments: &[String], input: impl BufRead, token: impl FnOnce(&str) -> Result<String, String>) -> Result<Option<String>, String> {
    let (confined, pinned) = match arguments {
        [.., action] if action == "store" || action == "erase" => return Ok(None),
        [mode, get] if mode == CONFINED && get == "get" => (true, None),
        [mode, own, get] if mode == LANE && get == "get" => (false, Some(own.as_str())),
        [get] if get == "get" => (false, None),
        _ => return Err(USAGE.to_owned()),
    };
    let (mut protocol, mut host, mut path) = (None, None, String::new());
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
    let token = if confined { Err("a confined worker command's https git reads no GitHub credential in its domain; `north lane sync CHECKOUT` fetches a main checkout, whose refs its lanes share, and `safe-push` pushes a lane, both outside the domain".to_owned()) } else if let Some(own) = pinned.filter(|own| !path.trim_matches('/').trim_end_matches(".git").eq_ignore_ascii_case(own)) { Err(format!("a session's lane push asks only for its own repository {own}, not {path}")) } else { token(&path) }?;
    Ok(Some(format!("username=x-access-token\npassword={token}\n")))
}

#[cfg(test)]
#[path = "git_credential_tests.rs"]
mod tests;
