use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use north_base::platform::process;

use crate::credentials::GITHUB_USER_FILE;
use crate::curl;

pub const GITHUB_API: &str = "https://api.github.com";
const LIMIT: Duration = Duration::from_secs(60);
const LONGEST_TOKEN: usize = 255;
const EXPIRY_HEADER: &str = "github-authentication-token-expiration";
const SCOPES_HEADER: &str = "x-oauth-scopes";
const SOURCE_LINE: &str = "source = \"secrets/github-user.yaml\"";

pub trait Clipboard {
    fn read(&self) -> Result<String, String>;
    fn clear(&self) -> Result<(), String>;
}

pub struct Wayland;

impl Clipboard for Wayland {
    fn read(&self) -> Result<String, String> {
        let output = Command::new("wl-paste").arg("--no-newline").output().map_err(|error| format!("cannot run wl-paste: {error}"))?;
        if !output.status.success() {
            return Err(format!("wl-paste {}", output.status));
        }
        String::from_utf8(output.stdout).map_err(|_| "the clipboard does not hold text".to_owned())
    }

    fn clear(&self) -> Result<(), String> {
        let status = Command::new("wl-copy").arg("--clear").status().map_err(|error| format!("cannot run wl-copy: {error}"))?;
        status.success().then_some(()).ok_or_else(|| format!("wl-copy --clear {status}"))
    }
}

pub struct Stored {
    pub expires: String,
    pub permissions: String,
}

pub fn run(secrets: &Path, api: &str, clipboard: &dyn Clipboard, out: &mut impl Write) -> Result<(), String> {
    let stored = store(secrets, api, clipboard)?;
    writeln!(out, "expires: {}\npermissions: {}", stored.expires, stored.permissions).map_err(|error| error.to_string())
}

pub fn store(secrets: &Path, api: &str, clipboard: &dyn Clipboard) -> Result<Stored, String> {
    let copied = clipboard.read()?;
    let token = copied.trim();
    well_formed(token)?;
    let registry = Registry::open(secrets)?;
    let stored = check(api, token)?;
    encrypt(secrets, token)?;
    let cleared = clipboard.clear();
    registry.record_expiry(&stored.expires)?;
    cleared.map_err(|why| format!("the token is stored, but the clipboard still holds it: {why}"))?;
    Ok(stored)
}

fn well_formed(token: &str) -> Result<(), String> {
    if token.is_empty() || token.len() > LONGEST_TOKEN || !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err("the clipboard does not hold a GitHub token (letters, digits, '_' and '-' only, up to 255 characters); nothing was stored".to_owned());
    }
    Ok(())
}

fn check(api: &str, token: &str) -> Result<Stored, String> {
    let config = format!("header = {}\nheader = \"Accept: application/vnd.github+json\"\nheader = \"User-Agent: north-broker\"\n", curl::quoted(&format!("Authorization: Bearer {token}"))?);
    let scheme = if api.starts_with("https://") { "=https" } else { "=http,https" };
    let raw = curl::run(&["--include", "--max-time", "30", "--proto", scheme, "--request", "GET", &format!("{api}/user")], &config, LIMIT).ok_or_else(|| format!("could not reach {api}; nothing was stored"))?;
    let text = String::from_utf8_lossy(&raw);
    let head = text.split("\r\n\r\n").next().unwrap_or_default();
    let mut lines = head.lines();
    let status = lines.next().and_then(|line| line.split_whitespace().nth(1)).unwrap_or("unreadable");
    if status != "200" {
        return Err(format!("GitHub answered HTTP {status} to the token; nothing was stored"));
    }
    let headers: Vec<(String, String)> = lines.filter_map(|line| line.split_once(':')).map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned())).collect();
    let header = |name: &str| headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());
    Ok(Stored {
        expires: header(EXPIRY_HEADER).unwrap_or_else(|| "none".to_owned()),
        permissions: header(SCOPES_HEADER).filter(|scopes| !scopes.is_empty()).unwrap_or_else(|| "none listed by GitHub (a fine-grained token's permissions are not reported)".to_owned()),
    })
}

fn encrypt(secrets: &Path, token: &str) -> Result<(), String> {
    let file = secrets.join(GITHUB_USER_FILE);
    let target = file.to_string_lossy();
    let mut command = Command::new("sops");
    if let Some(south) = secrets.parent() {
        command.current_dir(south);
    }
    command.args(["--encrypt", "--input-type", "yaml", "--output-type", "yaml", "--filename-override", target.as_ref(), "/dev/stdin"]);
    let output = process::try_run_command_within(command, Some(format!("token: {token}\n").as_bytes()), LIMIT).map_err(|error| format!("cannot run sops: {error}"))?.ok_or("sops did not finish in time")?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr).lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("no message").chars().take(160).collect::<String>();
        return Err(format!("sops {}: {why}; nothing was stored", output.status));
    }
    let staging = file.with_extension("yaml.part");
    std::fs::write(&staging, &output.stdout).map_err(|error| format!("cannot write {}: {error}", staging.display()))?;
    std::fs::rename(&staging, &file).map_err(|error| format!("cannot replace {}: {error}", file.display()))
}

struct Registry {
    path: PathBuf,
    text: String,
}

impl Registry {
    fn open(secrets: &Path) -> Result<Self, String> {
        let path = secrets.parent().map(|south| south.join("agents/credentials.toml")).ok_or("the secrets directory has no parent")?;
        let text = std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if !text.lines().any(|line| line.trim() == SOURCE_LINE) {
            return Err(format!("secrets/{GITHUB_USER_FILE} is not declared in {}; declare it with reach \"broker:github-user\" first; nothing was stored", path.display()));
        }
        Ok(Self { path, text })
    }

    fn record_expiry(self, expires: &str) -> Result<(), String> {
        let day: String = expires.chars().take(10).collect();
        let dated = day.len() == 10 && day.bytes().enumerate().all(|(at, byte)| if matches!(at, 4 | 7) { byte == b'-' } else { byte.is_ascii_digit() });
        let mut lines: Vec<String> = Vec::new();
        let mut inside = false;
        for line in self.text.lines() {
            if line.starts_with('[') {
                inside = false;
            }
            if inside && line.trim_start().starts_with("expires") {
                continue;
            }
            lines.push(line.to_owned());
            if line.trim() == SOURCE_LINE {
                inside = true;
                if dated {
                    lines.push(format!("expires = \"{day}\""));
                }
            }
        }
        std::fs::write(&self.path, lines.join("\n") + "\n").map_err(|error| format!("cannot write {}: {error}", self.path.display()))
    }
}
