use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::model::{Credential, Kind, day};
use super::{DAY, World, accounts, declared};

pub fn gather(world: &World) -> Result<Vec<Credential>, String> {
    let declarations = declared::read(&world.south)?;
    let mut found = declared::files(&world.south, &declarations)?;
    found.extend(declared::browser_sessions(&world.chrome_profile, &declarations, world.now));
    found.extend(accounts::gather(world)?);
    mark_copies(&mut found);
    found.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(found)
}

fn mark_copies(credentials: &mut [Credential]) {
    let mut owners: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, credential) in credentials.iter().enumerate() {
        if credential.source.ends_with(".credentials.json") && let Some(print) = &credential.fingerprint {
            owners.entry(print.clone()).or_default().push(index);
        }
    }
    for indices in owners.values().filter(|indices| indices.len() > 1) {
        let names: Vec<String> = indices.iter().map(|index| credentials[*index].name.clone()).collect();
        for index in indices {
            credentials[*index].note = Some(format!("copied login: one refresh token shared by {}", names.join(", ")));
        }
    }
}

pub fn failure(credential: &Credential, now: u64) -> Option<String> {
    match credential.kind {
        Kind::HumanSession => Some("human-session: only a browser sign-in renews it".into()),
        Kind::Retired => None,
        Kind::Undeclared => Some("undeclared: no kind in south agents/credentials.toml".into()),
        Kind::Machine => credential.expires.filter(|expires| *expires <= now).map(|expires| format!("expired on {}", day(expires))),
    }
}

pub fn failing(credentials: &[Credential], now: u64) -> Vec<(&Credential, String)> {
    credentials.iter().filter_map(|credential| failure(credential, now).map(|why| (credential, why))).collect()
}

fn expiry(credential: &Credential, now: u64) -> String {
    match credential.expires {
        None => "never".into(),
        Some(expires) if expires <= now => format!("{} (expired)", day(expires)),
        Some(expires) => format!("{} (in {} days)", day(expires), (expires - now) / DAY),
    }
}

pub fn lines(credentials: &[Credential], now: u64) -> Vec<String> {
    credentials
        .iter()
        .map(|credential| {
            let print = credential.fingerprint.as_deref().unwrap_or("-");
            let flag = failure(credential, now).map_or_else(String::new, |why| format!("  FAIL {why}"));
            let note = credential.note.as_deref().map_or_else(String::new, |note| format!("  [{note}]"));
            format!("{}  {}  {}  expires {}  renews: {}  use: {}{}{}", credential.name, credential.kind.label(), print, expiry(credential, now), credential.renewal.describe(), credential.usage, note, flag)
        })
        .collect()
}

pub fn json(credentials: &[Credential], now: u64) -> Value {
    let rows: Vec<Value> = credentials
        .iter()
        .map(|credential| {
            json!({
                "name": credential.name,
                "source": credential.source,
                "use": credential.usage,
                "kind": credential.kind.label(),
                "fingerprint": credential.fingerprint,
                "expires": credential.expires,
                "renewal": credential.renewal.describe(),
                "failing": failure(credential, now),
            })
        })
        .collect();
    json!({ "at": now, "credentials": rows })
}

