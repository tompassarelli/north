use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use super::broker_client::{AGENT_APP, Client, Grant};

const PUSHED_TOKEN: &str = ".local/state/north/gh-write-token";
const PUSHED_FRESH_SECONDS: i64 = super::pushed_token::FRESH_SECONDS;

/// The logins the agent GitHub App posts as: the name it had before North Fleet and the name after
/// the 2026-11-10 rename, so history the old bot wrote is still recognised.
pub const AGENT_LOGINS: [&str; 2] = ["socrates-agent[bot]", "north-fleet[bot]"];

fn pushed_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(PUSHED_TOKEN)).filter(|path| path.exists())
}

pub fn agent_installation(now: i64) -> Result<String, String> {
    agent_token(now, None, None).map(|(token, _)| token)
}

pub fn agent_token(now: i64, repository: Option<&str>, launched: Option<String>) -> Result<(String, Option<String>), String> {
    if let Some(pushed) = pushed_file() {
        let token = pushed_token(&pushed, now)?;
        return Ok((token, Some(format!("using the North Fleet App token the laptop pushed to {}", pushed.display()))));
    }
    let repositories: Vec<String> = repository.map(str::to_owned).into_iter().collect();
    match Client::discover().github_token(AGENT_APP, &repositories, &BTreeMap::new(), 0) {
        Ok(Grant { token, .. }) => Ok((token, None)),
        Err(why) => {
            let token = launched.filter(|token| token.starts_with("ghs_")).ok_or_else(|| why.clone())?;
            Ok((token, Some(format!("cannot get a North Fleet App token from the broker ({why}); using the session's GH_TOKEN"))))
        }
    }
}

fn pushed_token(pushed: &std::path::Path, now: i64) -> Result<String, String> {
    let shown = pushed.display();
    let written = fs::metadata(pushed).and_then(|meta| meta.modified()).map_err(|error| format!("cannot read {shown}: {error}"))?;
    let written = written.duration_since(std::time::UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX));
    let age = now - written;
    if !(0..PUSHED_FRESH_SECONDS).contains(&age) {
        return Err(format!("the token the laptop pushes to {shown} is {} min old, outside its 55 min trust; the laptop's host-gh-token timer has not refreshed it, and this host holds no App key to mint one", age / 60));
    }
    let token = fs::read_to_string(pushed).map_err(|error| format!("cannot read {shown}: {error}"))?.trim().to_owned();
    if token.is_empty() {
        return Err(format!("{shown} is empty"));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::AGENT_LOGINS;

    #[test]
    fn the_agent_app_answers_to_both_its_old_and_its_north_fleet_login() {
        assert!(AGENT_LOGINS.contains(&"socrates-agent[bot]"));
        assert!(AGENT_LOGINS.contains(&"north-fleet[bot]"));
    }
}
