use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

use super::model::{Credential, Kind, Renewal, fingerprint, modified};
use super::{DAY, World};

const TOKEN_LIFETIME_DAYS: u64 = 365;

struct Profile {
    account: String,
    path: PathBuf,
    token_file: Option<PathBuf>,
    other_hosts: Vec<String>,
}

fn expand(home: &Path, path: &str) -> PathBuf {
    path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest))
}

fn profiles(world: &World) -> Result<Vec<Profile>, String> {
    let source = std::fs::read_to_string(&world.orchestration).map_err(|error| format!("cannot read {}: {error}", world.orchestration.display()))?;
    let document: DocumentMut = source.parse().map_err(|error| format!("{}: {error}", world.orchestration.display()))?;
    let Some(accounts) = document.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("profiles")).and_then(Item::as_table) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for (account, hosts) in accounts {
        let Some(hosts) = hosts.as_table_like() else { continue };
        let entry = |host: &str, key: &str| hosts.get(host).and_then(|entry| entry.get(key)).and_then(Item::as_str).map(|path| expand(&world.home, path));
        let Some(path) = entry(&world.host, "path") else { continue };
        let other_hosts = hosts.iter().map(|(host, _)| host.to_owned()).filter(|host| *host != world.host).collect();
        found.push(Profile { account: account.to_owned(), path, token_file: entry(&world.host, "token_file"), other_hosts });
    }
    Ok(found)
}

fn token(world: &World, profile: &Profile, file: &Path) -> Credential {
    let account = &profile.account;
    let elsewhere = if profile.other_hosts.is_empty() { String::new() } else { format!("; its copies on {} are read there", profile.other_hosts.join(", ")) };
    let (print, expires, note) = match std::fs::read(file) {
        Ok(bytes) => (Some(fingerprint(&bytes)), modified(file).map(|issued| issued + TOKEN_LIFETIME_DAYS * DAY), None),
        Err(_) => (None, Some(world.now), Some("token file missing".to_owned())),
    };
    Credential {
        name: format!("claude:{account}:token"),
        source: file.display().to_string(),
        usage: format!("Claude account {account}: setup token a session on this host launches on when the host holds no login of its own{elsewhere}"),
        kind: Kind::Machine,
        fingerprint: print,
        expires,
        renewal: Renewal::Tom(format!("run claude setup-token for account {account} (one browser consent a year), then store it in south secrets/claude-tokens.yaml and ship it by sops, never by copying a credentials file")),
        note,
    }
}

fn login(profile: &Profile, file: &Path) -> Option<Credential> {
    let text = std::fs::read_to_string(file).ok()?;
    let oauth = serde_json::from_str::<serde_json::Value>(&text).ok()?.get("claudeAiOauth")?.clone();
    let account = &profile.account;
    Some(Credential {
        name: format!("claude:{account}:login"),
        source: file.display().to_string(),
        usage: format!("Claude account {account}: browser sign-in whose refresh token this host holds; north launches on it before the setup token"),
        kind: Kind::HumanSession,
        fingerprint: oauth.get("refreshToken").and_then(|token| token.as_str()).map(|token| fingerprint(token.as_bytes())),
        expires: oauth.get("refreshTokenExpiresAt").and_then(serde_json::Value::as_u64).map(|millis| millis / 1000),
        renewal: Renewal::Tom(format!("delete this login so account {account} launches on its setup token, or sign in again in a browser; never copy it from another host (one refresh token, one host)")),
        note: None,
    })
}

pub fn gather(world: &World) -> Result<Vec<Credential>, String> {
    let mut found = Vec::new();
    for profile in profiles(world)? {
        if let Some(file) = &profile.token_file {
            found.push(token(world, &profile, file));
        }
        found.extend(login(&profile, &profile.path.join(".credentials.json")));
    }
    Ok(found)
}
