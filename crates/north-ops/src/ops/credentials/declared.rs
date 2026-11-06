use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use toml_edit::{DocumentMut, Item, Table};

use super::model::{Credential, Kind, Renewal, fingerprint};

pub const FILE: &str = "agents/credentials.toml";
const CHROME_EPOCH_OFFSET_S: u64 = 11_644_473_600;

#[derive(Debug)]
pub struct Declaration {
    pub source: String,
    pub usage: String,
    pub kind: Kind,
    pub renewal: Renewal,
    pub expires: Option<u64>,
}

#[derive(Debug)]
pub struct Browser {
    pub host: String,
    pub usage: String,
    pub renewal: Renewal,
}

#[derive(Debug, Default)]
pub struct Declarations {
    pub files: Vec<Declaration>,
    pub browsers: Vec<Browser>,
}

fn text(table: &Table, key: &str) -> Result<String, String> {
    table.get(key).and_then(Item::as_str).map(str::to_owned).ok_or_else(|| format!("{FILE}: an entry has no {key}"))
}

pub fn renewal(spec: &str) -> Result<Renewal, String> {
    match spec.split_once(':') {
        None if spec.trim() == "never" => Ok(Renewal::Never),
        Some(("tom", cause)) if !cause.trim().is_empty() => Ok(Renewal::Tom(cause.trim().to_owned())),
        Some(("run", command)) if !command.trim().is_empty() => Ok(Renewal::Command(command.split_whitespace().map(str::to_owned).collect())),
        _ => Err(format!("{FILE}: renewal {spec:?} is not never, tom: CAUSE or run: COMMAND")),
    }
}

pub fn date(text: &str) -> Result<u64, String> {
    let bad = || format!("{FILE}: expires {text:?} is not YYYY-MM-DD");
    let parts: Vec<&str> = text.split('-').collect();
    let [year, month, day] = parts[..] else { return Err(bad()) };
    let month = time::Month::try_from(month.parse::<u8>().map_err(|_| bad())?).map_err(|_| bad())?;
    let date = time::Date::from_calendar_date(year.parse().map_err(|_| bad())?, month, day.parse().map_err(|_| bad())?).map_err(|_| bad())?;
    u64::try_from(date.midnight().assume_utc().unix_timestamp()).map_err(|_| bad())
}

pub fn parse(source: &str) -> Result<Declarations, String> {
    let document: DocumentMut = source.parse().map_err(|error| format!("{FILE}: {error}"))?;
    let mut found = Declarations::default();
    for table in document.get("credential").and_then(Item::as_array_of_tables).into_iter().flatten() {
        let kind = match text(table, "kind")?.as_str() {
            "machine" => Kind::Machine,
            "human-session" => Kind::HumanSession,
            other => return Err(format!("{FILE}: kind {other:?} is not machine or human-session")),
        };
        let expires = table.get("expires").and_then(Item::as_str).map(date).transpose()?;
        found.files.push(Declaration { source: text(table, "source")?, usage: text(table, "use")?, kind, renewal: renewal(&text(table, "renewal")?)?, expires });
    }
    for table in document.get("browser_session").and_then(Item::as_array_of_tables).into_iter().flatten() {
        found.browsers.push(Browser { host: text(table, "host")?, usage: text(table, "use")?, renewal: renewal(&text(table, "renewal")?)? });
    }
    Ok(found)
}

pub fn read(south: &Path) -> Result<Declarations, String> {
    let path = south.join(FILE);
    let source = std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse(&source)
}

fn sops_files(root: &Path, relative: &str, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(relative)) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{relative}/{name}");
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => sops_files(root, &path, found),
            Ok(kind) if kind.is_file() && name.ends_with(".yaml") => found.push(path),
            _ => {}
        }
    }
}

pub fn keys(sops_yaml: &str) -> Vec<String> {
    sops_yaml.lines().filter(|line| !line.starts_with([' ', '#', '-', '\t'])).filter_map(|line| line.split_once(':').map(|(key, _)| key.trim().to_owned())).filter(|key| !key.is_empty() && key != "sops").collect()
}

pub fn files(south: &Path, declarations: &Declarations) -> Result<Vec<Credential>, String> {
    let mut paths = Vec::new();
    sops_files(south, "secrets", &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|relative| {
            let bytes = std::fs::read(south.join(&relative)).map_err(|error| format!("cannot read {relative}: {error}"))?;
            let names = keys(&String::from_utf8_lossy(&bytes)).join(", ");
            let name = format!("sops:{}", relative.trim_start_matches("secrets/").trim_end_matches(".yaml"));
            let covering = declarations.files.iter().find(|declaration| declaration.source == relative || (declaration.source.ends_with('/') && relative.starts_with(&declaration.source)));
            Ok(match covering {
                Some(declaration) => Credential { name, source: relative, usage: format!("{} (keys: {names})", declaration.usage), kind: declaration.kind, fingerprint: Some(fingerprint(&bytes)), expires: declaration.expires, renewal: declaration.renewal.clone(), note: None },
                None => Credential { name, source: relative, usage: format!("keys: {names}"), kind: Kind::Undeclared, fingerprint: Some(fingerprint(&bytes)), expires: None, renewal: Renewal::Never, note: None },
            })
        })
        .collect()
}

fn cookie_expiry(database: &Path, host: &str) -> Option<Option<u64>> {
    let copy = tempfile::tempdir().ok()?;
    let local = copy.path().join("Cookies");
    std::fs::copy(database, &local).ok()?;
    let connection = Connection::open_with_flags(&local, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let latest: Option<i64> = connection
        .query_row("SELECT max(expires_utc) FROM cookies WHERE has_expires = 1 AND (host_key = ?1 OR host_key = '.' || ?1 OR host_key LIKE '%.' || ?1)", [host], |row| row.get(0))
        .ok()?;
    Some(latest.and_then(|micros| u64::try_from(micros / 1_000_000).ok()).and_then(|seconds| seconds.checked_sub(CHROME_EPOCH_OFFSET_S)))
}

pub fn browser_sessions(profile: &Path, declarations: &Declarations, now: u64) -> Vec<Credential> {
    let database: PathBuf = profile.join("Default/Cookies");
    if !database.is_file() {
        return Vec::new();
    }
    declarations
        .browsers
        .iter()
        .map(|browser| {
            let (expires, note) = match cookie_expiry(&database, &browser.host) {
                Some(Some(expires)) => (Some(expires), None),
                Some(None) => (Some(now), Some("no persistent session cookie: signed out".to_owned())),
                None => (None, Some("cookie store unreadable".to_owned())),
            };
            Credential { name: format!("browser:{}", browser.host), source: "automation-chrome profile".into(), usage: browser.usage.clone(), kind: Kind::HumanSession, fingerprint: None, expires, renewal: browser.renewal.clone(), note }
        })
        .collect()
}
