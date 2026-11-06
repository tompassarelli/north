use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde_json::{Value, json};

use super::process;
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north gh-app-token [--app socrates|lander] [--repos NAME,NAME [--permissions JSON]]

Print an installation access token for the Socrates Agent GitHub App, or with --app lander for the
north-lander App (secrets/nexus/north-lander.yaml, field pem, cached apart from the default).
With --repos, the token is limited to those repositories and contents write, and is not cached.
With --permissions, a JSON object of permission levels is added over contents write and metadata
read; a workflows permission is refused.
Credentials: ${SOUTH_SECRETS:-$HOME/code/south/main/secrets}/github-app.yaml (sops).
API base: ${GH_APP_API:-https://api.github.com}. Cache: ${XDG_RUNTIME_DIR}/gh-app-token,
reused while more than 5 minutes remain. A host the laptop pushes a token to
(~/.local/state/north/gh-write-token, as on gex45 and ccx33) holds no App key: it prints that token
while it is under 55 minutes old and never decrypts the credentials.";

const REUSE_MARGIN: i64 = 300;
const EXCHANGE_LIMIT: Duration = Duration::from_secs(30);
const DECRYPT_LIMIT: Duration = Duration::from_secs(60);
const PUSHED_TOKEN: &str = ".local/state/north/gh-write-token";
const PUSHED_FRESH_SECONDS: i64 = 55 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct App {
    file: &'static str,
    pem: &'static str,
    cache: &'static str,
}

const APPS: &[(&str, App)] = &[
    ("socrates", App { file: "github-app.yaml", pem: "private_key", cache: "gh-app-token" }),
    ("lander", App { file: "nexus/north-lander.yaml", pem: "pem", cache: "gh-app-token-lander" }),
];

pub const AGENT_APP: &str = "socrates";

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub app: App,
    pub repos: Option<Vec<String>>,
    pub permissions: BTreeMap<String, String>,
}

impl Request {
    pub fn installation(app_name: &str) -> Option<Self> {
        let app = APPS.iter().find(|(name, _)| *name == app_name).map(|(_, app)| *app)?;
        Some(Self { app, repos: None, permissions: default_permissions() })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Help,
    Mint(Request),
}

fn parse(arguments: &[String]) -> Result<Parsed, String> {
    let mut app_name = "socrates";
    let mut repos = None;
    let mut permissions = default_permissions();
    let mut granted = false;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if word == "-h" || word == "--help" {
            return Ok(Parsed::Help);
        }
        match (word.as_str(), words.next()) {
            ("--app", Some(value)) => app_name = value,
            ("--repos", Some(value)) => {
                let names: Vec<String> = value.split(',').filter(|name| !name.is_empty()).map(str::to_owned).collect();
                if names.is_empty() {
                    return Err("--repos needs at least one repository name".to_owned());
                }
                repos = Some(names);
            }
            ("--permissions", Some(value)) => {
                permissions.extend(requested_permissions(value)?);
                granted = true;
            }
            _ => return Err("takes --app socrates|lander, --repos NAME,NAME and --permissions JSON".to_owned()),
        }
    }
    if granted && repos.is_none() {
        return Err("--permissions needs --repos".to_owned());
    }
    let app = APPS.iter().find(|(name, _)| *name == app_name).map(|(_, app)| *app).ok_or_else(|| format!("unknown app {app_name}"))?;
    Ok(Parsed::Mint(Request { app, repos, permissions }))
}

fn default_permissions() -> BTreeMap<String, String> {
    BTreeMap::from([("contents".to_owned(), "write".to_owned()), ("metadata".to_owned(), "read".to_owned())])
}

fn requested_permissions(value: &str) -> Result<BTreeMap<String, String>, String> {
    let requested: BTreeMap<String, String> = serde_json::from_str(value).map_err(|_| "--permissions takes a JSON object of permission levels".to_owned())?;
    if requested.contains_key("workflows") {
        return Err("--permissions may not grant workflows".to_owned());
    }
    Ok(requested)
}

pub type Decrypt = Box<dyn Fn(&Path) -> Option<Vec<u8>>>;

pub struct Host {
    pub secrets: PathBuf,
    pub runtime: Option<PathBuf>,
    pub pushed: Option<PathBuf>,
    pub api: String,
    pub decrypt: Decrypt,
}

impl Host {
    pub fn from_env() -> Self {
        let var = |name| std::env::var(name).ok().filter(|value: &String| !value.is_empty());
        let secrets = var("SOUTH_SECRETS").map_or_else(|| Path::new(&var("HOME").unwrap_or_default()).join("code/south/main/secrets"), PathBuf::from);
        let api = var("GH_APP_API").unwrap_or_else(|| "https://api.github.com".to_owned()).trim_end_matches('/').to_owned();
        let key_file = std::env::var("SOPS_AGE_KEY_FILE").unwrap_or_else(|_| "/var/lib/sops-nix/key.txt".to_owned());
        let pushed = var("HOME").map(|home| Path::new(&home).join(PUSHED_TOKEN));
        Self { secrets, runtime: var("XDG_RUNTIME_DIR").map(PathBuf::from), pushed, api, decrypt: Box::new(move |file| sops_decrypt(&key_file, file)) }
    }

    pub fn pushed_file(&self) -> Option<&Path> {
        self.pushed.as_deref().filter(|path| path.exists())
    }
}

fn sops_decrypt(key_file: &str, file: &Path) -> Option<Vec<u8>> {
    let readable = fs::File::open(key_file).is_ok();
    let mut command = if readable { Command::new("sops") } else { Command::new("sudo") };
    if !readable {
        command.args(["-n", "env", &format!("SOPS_AGE_KEY_FILE={key_file}"), "sops"]);
    }
    command.args(["-d", "--output-type", "json"]).arg(file).env("SOPS_AGE_KEY_FILE", key_file);
    match process::run_command_within(command, None, DECRYPT_LIMIT) {
        Some(output) if output.status.success() => Some(output.stdout),
        _ => None,
    }
}

pub fn run(arguments: &[String]) -> u8 {
    run_on(arguments, Host::from_env, Timestamp::now().unix_seconds())
}

fn run_on(arguments: &[String], host: impl FnOnce() -> Host, now: i64) -> u8 {
    let request = match parse(arguments) {
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return 0;
        }
        Ok(Parsed::Mint(request)) => request,
        Err(message) => return fail(&message),
    };
    match mint(&request, &host(), now) {
        Ok(token) => {
            println!("{token}");
            0
        }
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> u8 {
    eprintln!("gh-app-token: {message}");
    1
}

pub fn mint(request: &Request, host: &Host, now: i64) -> Result<String, String> {
    if let Some(pushed) = host.pushed_file() {
        return pushed_token(request, pushed, now);
    }
    let app = request.app;
    let cache = request.repos.is_none().then(|| host.runtime.as_ref().map(|runtime| runtime.join(app.cache))).flatten();
    if let Some(token) = cache.as_deref().and_then(|path| cached(path, now)) {
        return Ok(token);
    }
    let file = host.secrets.join(app.file);
    if !file.exists() {
        return Err(format!("missing {}", file.display()));
    }
    let decrypted = (host.decrypt)(&file).ok_or_else(|| format!("could not decrypt {}", app.file))?;
    let creds: Value = serde_json::from_slice(&decrypted).map_err(|_| format!("decrypted {} is not valid", app.file))?;
    let field = |name: &str| match &creds[name] {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    };
    let (app_id, installation_id) = (field("app_id"), field("installation_id"));
    let private_key = creds[app.pem].as_str().unwrap_or_default();
    if app_id.is_empty() || installation_id.is_empty() || private_key.is_empty() {
        return Err(format!("{} lacks app_id, installation_id or {}", app.file, app.pem));
    }
    let jwt = app_jwt(&app_id, private_key, now).ok_or("could not sign the app JWT")?;
    let scope = request.repos.as_deref().map(|repos| (repos, &request.permissions));
    let (token, expires_at) = exchange(host, &installation_id, &jwt, scope)?;
    if let Some(path) = cache {
        store(&path, &token, &expires_at).map_err(|_| format!("could not write {}", path.display()))?;
    }
    Ok(token)
}

fn cached(path: &Path, now: i64) -> Option<String> {
    let entry: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let expires = Timestamp::parse_rfc3339(entry["expires_at"].as_str()?)?.unix_seconds();
    (expires - now > REUSE_MARGIN).then(|| entry["token"].as_str().map(str::to_owned)).flatten()
}

fn store(path: &Path, token: &str, expires_at: &str) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&temporary)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(json!({ "token": token, "expires_at": expires_at }).to_string().as_bytes())?;
    fs::rename(&temporary, path)
}

fn app_jwt(app_id: &str, pem: &str, now: i64) -> Option<String> {
    let issued = now - 60;
    let head = URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256", "typ": "JWT" }).to_string());
    let body = URL_SAFE_NO_PAD.encode(json!({ "iss": app_id, "iat": issued, "exp": issued + 600 }).to_string());
    let signing_input = format!("{head}.{body}");
    let key = signing_key(pem)?;
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), signing_input.as_bytes(), &mut signature).ok()?;
    Some(format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

fn signing_key(pem: &str) -> Option<RsaKeyPair> {
    let pkcs1 = pem.contains("BEGIN RSA PRIVATE KEY");
    let body: String = pem.lines().filter(|line| !line.starts_with("-----")).map(str::trim).collect();
    let der = STANDARD.decode(body).ok()?;
    if pkcs1 { RsaKeyPair::from_der(&der) } else { RsaKeyPair::from_pkcs8(&der) }.ok()
}

fn exchange(host: &Host, installation_id: &str, jwt: &str, scope: Option<(&[String], &BTreeMap<String, String>)>) -> Result<(String, String), String> {
    let installation = percent_encoding::utf8_percent_encode(installation_id, percent_encoding::NON_ALPHANUMERIC);
    let url = format!("{}/app/installations/{installation}/access_tokens", host.api);
    let mut command = Command::new("curl");
    command.args(["--silent", "--show-error", "--max-time", "30", "--request", "POST", "--config", "-", "--write-out", "\n%{http_code}"]);
    if let Some((repos, permissions)) = scope {
        command.args(["--data-binary", &json!({ "repositories": repos, "permissions": permissions }).to_string()]);
    }
    command.arg(&url);
    let headers = format!(
        "header = \"Authorization: Bearer {jwt}\"\nheader = \"Accept: application/vnd.github+json\"\nheader = \"X-GitHub-Api-Version: 2022-11-28\"\nuser-agent = \"gh-app-token\"\n"
    );
    let unreachable = || format!("could not reach {}", host.api);
    let output = process::run_command_within(command, Some(headers.as_bytes()), EXCHANGE_LIMIT).ok_or_else(unreachable)?;
    let text = String::from_utf8_lossy(&output.stdout);
    let (body, status) = text.rsplit_once('\n').ok_or_else(unreachable)?;
    let status: u16 = status.trim().parse().map_err(|_| unreachable())?;
    if !output.status.success() || status == 0 {
        return Err(unreachable());
    }
    let data: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let message = data["message"].as_str().map(|message| format!(" {}", message.chars().take(200).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "))).unwrap_or_default();
        return Err(format!("token exchange failed: HTTP {status}{message}"));
    }
    match (data["token"].as_str(), data["expires_at"].as_str()) {
        (Some(token), Some(expires_at)) if Timestamp::parse_rfc3339(expires_at).is_some() => Ok((token.to_owned(), expires_at.to_owned())),
        _ => Err("token exchange returned no token".to_owned()),
    }
}

fn pushed_token(request: &Request, pushed: &Path, now: i64) -> Result<String, String> {
    let shown = pushed.display();
    let agent = APPS.iter().any(|(name, app)| *name == AGENT_APP && *app == request.app);
    if !agent || request.repos.is_some() {
        return Err(format!("this host takes the App token the laptop pushes to {shown} and holds no App key to mint another"));
    }
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
#[path = "github_app_tests.rs"]
mod tests;
