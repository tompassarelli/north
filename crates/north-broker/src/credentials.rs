use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use north_base::platform::{process, shared_config};
use serde_json::Value;

const DECRYPT_LIMIT: Duration = Duration::from_secs(60);

pub const AGENT_APP_FILE: &str = "github-app.yaml";
pub const LANDER_APP_FILE: &str = "north-lander.yaml";
pub const DIGITALOCEAN_FILE: &str = "digitalocean.yaml";
pub const HETZNER_FILE: &str = "hetzner-cloud.yaml";
pub const CLOUDFLARE_FILE: &str = "cloudflare.yaml";
pub const GITHUB_USER_FILE: &str = "github-user.yaml";
pub const INSTALLATION_TOKEN_FILE: &str = "github-installation-token";
pub const LEDGER_KEY_FILE: &str = "threads-ledger.key";

pub enum Source {
    Directory(PathBuf),
    Sops(PathBuf),
}

impl Source {
    pub fn systemd() -> Result<Self, String> {
        std::env::var_os("CREDENTIALS_DIRECTORY").filter(|directory| !directory.is_empty()).map(|directory| Self::Directory(directory.into())).ok_or_else(|| "CREDENTIALS_DIRECTORY is unset; run under a systemd unit that loads each secret as a credential".to_owned())
    }

    pub fn south() -> Self {
        let var = |name| std::env::var_os(name).filter(|value: &std::ffi::OsString| !value.is_empty());
        let secrets = var("SOUTH_SECRETS").map(PathBuf::from).unwrap_or_else(|| shared_config::checkout(&PathBuf::from(var("HOME").unwrap_or_default())).join("secrets"));
        Self::Sops(secrets)
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        match self {
            Self::Directory(directory) => Some(directory.join(name)).filter(|path| path.is_file()),
            Self::Sops(_) => None,
        }
    }

    pub fn secret_values(&self, known: &str) -> Vec<String> {
        let files = [AGENT_APP_FILE, LANDER_APP_FILE, DIGITALOCEAN_FILE, HETZNER_FILE, CLOUDFLARE_FILE, GITHUB_USER_FILE, INSTALLATION_TOKEN_FILE];
        let mut values: Vec<String> = files.iter().filter_map(|name| self.fields(name).ok()).flat_map(|fields| fields.into_values()).collect();
        values.push(known.to_owned());
        values
    }

    pub fn fields(&self, name: &str) -> Result<BTreeMap<String, String>, String> {
        let text = match self {
            Self::Directory(directory) => std::fs::read_to_string(directory.join(name)).map_err(|error| format!("credential {name} is not loaded: {error}"))?,
            Self::Sops(secrets) => decrypt(secrets, name)?,
        };
        parse(&text).ok_or_else(|| format!("credential {name} is not JSON or flat YAML"))
    }
}

fn declared_for_broker(secrets: &std::path::Path, file: &std::path::Path) -> Result<(), String> {
    let registry = secrets.parent().map(|south| south.join("agents/credentials.toml")).ok_or("the secrets directory has no parent")?;
    let text = std::fs::read_to_string(&registry).map_err(|error| format!("cannot read {} to confirm the broker may load {}: {error}", registry.display(), file.display()))?;
    let document: toml_edit::DocumentMut = text.parse().map_err(|error| format!("{}: {error}", registry.display()))?;
    let relative = format!("secrets/{}", file.strip_prefix(secrets).map_err(|error| error.to_string())?.display());
    let reach = document.get("credential").and_then(toml_edit::Item::as_array_of_tables).into_iter().flatten().find(|entry| entry.get("source").and_then(toml_edit::Item::as_str) == Some(relative.as_str())).and_then(|entry| entry.get("reach")).and_then(toml_edit::Item::as_str);
    match reach {
        Some(reach) if reach.starts_with("broker:") => Ok(()),
        _ => Err(format!("{relative} is not declared reach = \"broker:...\" in {}; the broker loads only broker secrets", registry.display())),
    }
}

fn first_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).lines().map(str::trim).find(|line| !line.is_empty()).map_or_else(|| "no message".to_owned(), |line| line.chars().take(160).collect())
}

fn decrypt(secrets: &std::path::Path, name: &str) -> Result<String, String> {
    let candidates: Vec<PathBuf> = [secrets.join("nexus").join(name), secrets.join(name)].into_iter().filter(|path| path.exists()).collect();
    if candidates.is_empty() {
        return Err(format!("no secret {name} under {}", secrets.display()));
    }
    let mut last = format!("could not decrypt {name}");
    for file in candidates {
        if let Err(why) = declared_for_broker(secrets, &file) {
            last = why;
            continue;
        }
        let mut command = Command::new("sops");
        command.args(["-d", "--output-type", "json"]).arg(&file);
        match process::try_run_command_within(command, None, DECRYPT_LIMIT) {
            Ok(Some(output)) if output.status.success() => return String::from_utf8(output.stdout).map_err(|_| format!("decrypted {name} is not text")),
            Ok(Some(output)) => last = format!("could not decrypt {name}: sops {}: {}", output.status, first_line(&output.stderr)),
            Ok(None) => last = format!("could not decrypt {name}: sops did not finish within {} s", DECRYPT_LIMIT.as_secs()),
            Err(error) => last = format!("could not decrypt {name}: cannot run sops: {error}"),
        }
    }
    Err(last)
}

pub fn parse(text: &str) -> Option<BTreeMap<String, String>> {
    if text.trim_start().starts_with('{') {
        let Value::Object(map) = serde_json::from_str(text).ok()? else { return None };
        return Some(map.into_iter().filter_map(|(key, value)| match value {
            Value::String(text) => Some((key, text)),
            Value::Number(number) => Some((key, number.to_string())),
            _ => None,
        }).collect());
    }
    let mut fields = BTreeMap::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, value)) = line.split_once(':').filter(|(key, _)| !line.starts_with([' ', '#', '-']) && !key.is_empty()) else { continue };
        let value = value.trim();
        if value == "|" || value == "|-" || value == "|+" {
            let mut block = Vec::new();
            while let Some(next) = lines.next_if(|next| next.is_empty() || next.starts_with(' ')) {
                block.push(next.trim_start());
            }
            fields.insert(key.to_owned(), format!("{}\n", block.join("\n").trim_end()));
        } else {
            fields.insert(key.to_owned(), value.trim_matches(['"', '\'']).to_owned());
        }
    }
    Some(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decrypted_yaml_credential_keeps_its_key_block_and_a_json_one_reads_alike() {
        let (begin, end) = (["-----BEGIN", "KEY-----"].join(" PRIVATE "), ["-----END", "KEY-----"].join(" PRIVATE "));
        let yaml = format!("app_id: 123\ninstallation_id: \"42\"\nprivate_key: |\n    {begin}\n    abc\n    {end}\n");
        let json = serde_json::json!({ "app_id": 123, "installation_id": "42", "private_key": format!("{begin}\nabc\n{end}\n") }).to_string();
        assert_eq!(parse(&yaml), parse(&json));
        assert_eq!(parse(&yaml).unwrap()["private_key"], format!("{begin}\nabc\n{end}\n"));
    }

    #[test]
    fn a_secret_the_registry_does_not_give_broker_reach_is_never_decrypted() {
        let south = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(south.path().join("secrets")).unwrap();
        std::fs::create_dir_all(south.path().join("agents")).unwrap();
        std::fs::write(south.path().join("secrets/aws.yaml"), "x: y\n").unwrap();
        std::fs::write(south.path().join("agents/credentials.toml"), "[[credential]]\nsource = \"secrets/aws.yaml\"\nreach = \"operator\"\n").unwrap();
        let error = Source::Sops(south.path().join("secrets")).fields("aws.yaml").unwrap_err();
        assert!(error.contains("loads only broker secrets"), "{error}");
    }
}
