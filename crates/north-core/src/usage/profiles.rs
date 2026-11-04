//! Claude profiles the policy declares for this host: where each lives, which plan it signs in with, and the
//! projection of the shared harness into each profile directory.
use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub(super) const PROJECTED: [&str; 6] = ["CLAUDE.md", "settings.json", "hooks", "skills", "agents", "commands"];

pub struct Profile {
    pub label: String,
    pub path: Option<PathBuf>,
    pub token_file: bool,
}

pub fn policy_value(policy: &Path) -> Value {
    let document = fs::read_to_string(policy).ok().and_then(|text| text.parse::<toml_edit::DocumentMut>().ok());
    document.map_or(Value::Null, |document| crate::router::policy::json_value(document.as_item()))
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

pub fn profiles(cfg: &Value, host: &str) -> Vec<Profile> {
    cfg["accounts"]["claude"]["profiles"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(label, hosts)| {
            let profile = hosts.get(host)?;
            Some(Profile {
                label: label.clone(),
                path: profile.get("path").and_then(Value::as_str).map(expand),
                token_file: profile.get("token_file").is_some(),
            })
        })
        .collect()
}

pub fn label_of(cfg: &Value, uuid: &str) -> Option<String> {
    cfg["accounts"]["claude"]["labels"][short_hash(uuid)].as_str().map(str::to_owned)
}

pub fn short_hash(uuid: &str) -> String {
    Sha256::digest(uuid.as_bytes()).iter().take(4).map(|byte| format!("{byte:02x}")).collect()
}

fn read(path: &Path, key: &str) -> Value {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).map_or(Value::Null, |data| data[key].clone())
}

pub fn observed_plan(home: &Path, directory: Option<&Path>) -> String {
    let account = read(&directory.unwrap_or(home).join(".claude.json"), "oauthAccount");
    let oauth = read(&directory.map_or_else(|| home.join(".claude"), Path::to_path_buf).join(".credentials.json"), "claudeAiOauth");
    let text = |value: &Value, key: &str| value[key].as_str().filter(|text| !text.is_empty()).map(str::to_owned);
    match text(&account, "organizationRateLimitTier").or_else(|| text(&oauth, "rateLimitTier")).as_deref() {
        Some("default_claude_max_5x") => "max-5x".into(),
        Some("default_claude_max_20x") => "max-20x".into(),
        _ => text(&oauth, "subscriptionType").unwrap_or_else(|| "unknown".into()),
    }
}

pub fn sync(cfg: &Value, host: &str, home: &Path) -> Result<Vec<String>, String> {
    let source = home.join(".claude");
    let source_config: Value = fs::read_to_string(home.join(".claude.json")).ok().map_or(Ok(json!({})), |text| serde_json::from_str(&text)).map_err(|error| error.to_string())?;
    let trusted: Vec<&String> = source_config["projects"].as_object().into_iter().flatten().filter(|(_, data)| data["hasTrustDialogAccepted"] == json!(true)).map(|(project, _)| project).collect();
    let mut said = Vec::new();
    for profile in profiles(cfg, host) {
        let Some(directory) = profile.path.filter(|directory| *directory != source) else { continue };
        fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        trust(&directory, &trusted)?;
        for name in PROJECTED {
            project(&source, &directory, name)?;
        }
        said.push(format!("Claude profile {}: harness projected to {}", profile.label, directory.display()));
    }
    Ok(said)
}

fn trust(directory: &Path, trusted: &[&String]) -> Result<(), String> {
    let path = directory.join(".claude.json");
    let mut data: Value = match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?,
        Err(_) => json!({}),
    };
    let mut changed = false;
    for project in trusted {
        let projects = data.as_object_mut().ok_or_else(|| format!("{} is not an object", path.display()))?.entry("projects").or_insert_with(|| json!({}));
        let consent = projects.as_object_mut().ok_or("projects is not an object")?.entry((*project).clone()).or_insert_with(|| Value::Object(Map::new()));
        if consent["hasTrustDialogAccepted"] != json!(true) {
            consent["hasTrustDialogAccepted"] = json!(true);
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    let mut pending = tempfile::Builder::new().prefix(".claude.json.").tempfile_in(directory).map_err(|error| error.to_string())?;
    pending.write_all(serde_json::to_string_pretty(&data).map_err(|error| error.to_string())?.as_bytes()).map_err(|error| error.to_string())?;
    if let Ok(existing) = fs::metadata(&path) {
        fs::set_permissions(pending.path(), fs::Permissions::from_mode(existing.permissions().mode())).map_err(|error| error.to_string())?;
    }
    pending.persist(&path).map_err(|error| error.to_string())?;
    Ok(())
}

fn project(source: &Path, directory: &Path, name: &str) -> Result<(), String> {
    let target = source.join(name);
    if !target.exists() {
        return Ok(());
    }
    let link = directory.join(name);
    if fs::read_link(&link).is_ok_and(|current| current == target) {
        return Ok(());
    }
    if link.symlink_metadata().is_ok() {
        let backup = directory.join(format!("{name}.before-harness-sync"));
        if backup.symlink_metadata().is_ok() {
            return Err(format!("profile projection backup already exists: {}", backup.display()));
        }
        fs::rename(&link, &backup).map_err(|error| error.to_string())?;
    }
    let temporary = directory.join(format!("{name}.harness-sync"));
    symlink(&target, &temporary).map_err(|error| error.to_string())?;
    fs::rename(&temporary, &link).map_err(|error| error.to_string())
}
