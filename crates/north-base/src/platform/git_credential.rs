use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::github_app::{self, AGENT_APP, Host, Request};
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north git-credential get|store|erase

The git credential helper agent sessions push to GitHub through: for https://github.com it answers
get with an installation token of the Socrates Agent App (north gh-app-token), so GitHub's tag
rulesets, which only the admin role and that App's v* release path bypass, bind agent pushes.
A host that cannot mint it (gex45 never decrypts the App key) answers with the same App's token
the session was launched with in GH_TOKEN, else the one pushed to the host,
~/.local/state/north/gh-write-token, while it is fresh and its repository list covers the
request's path, and names on stderr the token it used. Other hosts, store and erase get no
answer. Fleet sessions set it through GIT_CONFIG_COUNT, so Tom's own git and gh login stay unchanged.";

const SCOPE: &str = "credential.https://github.com.helper";
const HELPER: &str = "!north git-credential";
const PATH_SCOPE: &str = "credential.https://github.com.useHttpPath";
const PUSHED_TOKEN: &str = ".local/state/north/gh-write-token";
const PUSHED_REPOS: &str = ".local/state/north/gh-write-token.repos";
/// The host push refreshes the token well inside its 60 min life; south's gh-write-token-covers trusts it for 55 min.
const PUSHED_FRESH: Duration = Duration::from_secs(55 * 60);

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
    let home = std::env::var_os("HOME").unwrap_or_default();
    let token = |path: &str| {
        let request = Request::installation(AGENT_APP).ok_or_else(|| format!("unknown app {AGENT_APP}"))?;
        github_app::mint(&request, &Host::from_env(), Timestamp::now().unix_seconds()).or_else(|why| {
            let (source, token) = match session_token(std::env::var("GH_TOKEN").ok()) {
                Some(token) => ("the session's GH_TOKEN", Some(token)),
                None => ("the host's pushed ~/.local/state/north/gh-write-token", pushed_token(Path::new(&home), path, SystemTime::now())),
            };
            let Some(token) = token else { return Err(why) };
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

/// The installation token the launcher gave this session, for a host that holds no sops key to mint a fresh one from.
fn session_token(token: Option<String>) -> Option<String> {
    token.filter(|token| token.starts_with("ghs_"))
}

fn pushed_token(home: &Path, path: &str, now: SystemTime) -> Option<String> {
    let token = home.join(PUSHED_TOKEN);
    let age = now.duration_since(fs::metadata(&token).ok()?.modified().ok()?).unwrap_or_default();
    let repo = path.trim_end_matches('/').trim_end_matches(".git").to_lowercase();
    let covered = !repo.is_empty() && fs::read_to_string(home.join(PUSHED_REPOS)).ok()?.lines().any(|line| line.trim() == repo);
    let token = fs::read_to_string(&token).ok()?.trim().to_owned();
    (age < PUSHED_FRESH && covered && !token.is_empty()).then_some(token)
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
