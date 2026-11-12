use std::collections::BTreeMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use north_base::foundation::clock::Timestamp;
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde_json::{Value, json};

use crate::curl;
use crate::credentials::{AGENT_APP_FILE, INSTALLATION_TOKEN_FILE, LANDER_APP_FILE, Source};
use crate::policy::{Permissions, Scope};

const EXCHANGE_LIMIT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct App {
    pub file: &'static str,
    pub pem: &'static str,
}

pub fn app(name: &str) -> Option<App> {
    match name {
        "agent" => Some(App { file: AGENT_APP_FILE, pem: "private_key" }),
        "lander" => Some(App { file: LANDER_APP_FILE, pem: "pem" }),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Minted {
    pub token: String,
    pub expires_at: String,
    pub repositories: Option<Vec<String>>,
    pub permissions: Permissions,
}

pub struct Minter {
    pub api: String,
    pub source: Source,
}

impl Minter {
    pub fn new(source: Source) -> Self {
        Self { api: "https://api.github.com".to_owned(), source }
    }

    pub fn mint(&self, app: App, scope: &Scope, now: i64) -> Result<Minted, String> {
        if app.file == AGENT_APP_FILE && let Some(minted) = self.installation_token(scope, now)? {
            return Ok(minted);
        }
        let creds = self.source.fields(app.file)?;
        let (app_id, installation_id) = (creds.get("app_id").cloned().unwrap_or_default(), creds.get("installation_id").cloned().unwrap_or_default());
        let private_key = creds.get(app.pem).map(String::as_str).unwrap_or_default();
        if app_id.is_empty() || installation_id.is_empty() || private_key.is_empty() {
            return Err(format!("{} lacks app_id, installation_id or {}", app.file, app.pem));
        }
        let jwt = app_jwt(&app_id, private_key, now).ok_or("could not sign the app JWT")?;
        let data = self.exchange(&installation_id, &jwt, scope)?;
        let (Some(token), Some(expires_at)) = (data["token"].as_str(), data["expires_at"].as_str().filter(|at| Timestamp::parse_rfc3339(at).is_some())) else {
            return Err("token exchange returned no token".to_owned());
        };
        let repositories = (!scope.repositories.is_empty()).then(|| data["repositories"].as_array().map_or_else(|| scope.repositories.clone(), |list| list.iter().filter_map(|repository| repository["name"].as_str().map(str::to_owned)).collect()));
        let permissions = data["permissions"].as_object().map_or_else(|| scope.permissions.clone(), |map| map.iter().filter_map(|(name, level)| Some((name.clone(), level.as_str()?.to_owned()))).collect::<BTreeMap<_, _>>());
        Ok(Minted { token: token.to_owned(), expires_at: expires_at.to_owned(), repositories, permissions })
    }

    fn installation_token(&self, scope: &Scope, now: i64) -> Result<Option<Minted>, String> {
        if self.source.path(INSTALLATION_TOKEN_FILE).is_none() {
            return Ok(None);
        }
        let fields = self.source.fields(INSTALLATION_TOKEN_FILE)?;
        let token = fields.get("token").filter(|token| !token.is_empty()).ok_or_else(|| format!("{INSTALLATION_TOKEN_FILE} holds no token"))?;
        let expires_at = fields.get("expires_at").ok_or_else(|| format!("{INSTALLATION_TOKEN_FILE} holds no expires_at"))?;
        let expiry = Timestamp::parse_rfc3339(expires_at).ok_or_else(|| format!("{INSTALLATION_TOKEN_FILE} expires_at is not RFC 3339"))?;
        if expiry.unix_seconds() <= now {
            return Err(format!("{INSTALLATION_TOKEN_FILE} expired at {expires_at}"));
        }
        let repositories = (!scope.repositories.is_empty()).then(|| scope.repositories.clone());
        Ok(Some(Minted { token: token.clone(), expires_at: expires_at.clone(), repositories, permissions: scope.permissions.clone() }))
    }

    fn exchange(&self, installation_id: &str, jwt: &str, scope: &Scope) -> Result<Value, String> {
        let installation = percent_encoding::utf8_percent_encode(installation_id, percent_encoding::NON_ALPHANUMERIC);
        let url = format!("{}/app/installations/{installation}/access_tokens", self.api);
        let mut body = serde_json::Map::new();
        if !scope.repositories.is_empty() {
            body.insert("repositories".to_owned(), json!(scope.repositories));
        }
        if !scope.permissions.is_empty() {
            body.insert("permissions".to_owned(), json!(scope.permissions));
        }
        let headers = format!("header = \"Authorization: Bearer {jwt}\"\nheader = \"Accept: application/vnd.github+json\"\nheader = \"X-GitHub-Api-Version: 2022-11-28\"\nuser-agent = \"north-broker\"\n");
        let unreachable = || format!("could not reach {}", self.api);
        let body_option = (!body.is_empty()).then(|| Value::Object(body).to_string());
        let mut arguments = vec!["--max-time", "30", "--request", "POST", "--write-out", "\n%{http_code}"];
        let scheme = if self.api.starts_with("https://") { "=https" } else { "=http,https" };
        arguments.extend(["--proto", scheme]);
        let config = match &body_option {
            Some(text) => format!("{headers}data-raw = {}\n", curl::quoted(text)?),
            None => headers,
        };
        arguments.push(&url);
        let output = curl::run(&arguments, &config, EXCHANGE_LIMIT).ok_or_else(unreachable)?;
        let text = String::from_utf8_lossy(&output);
        let (body, status) = text.rsplit_once('\n').ok_or_else(unreachable)?;
        let status: u16 = status.trim().parse().map_err(|_| unreachable())?;
        if status == 0 {
            return Err(unreachable());
        }
        let data: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let message = data["message"].as_str().map(|message| format!(" {}", message.chars().take(200).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "))).unwrap_or_default();
            return Err(format!("token exchange failed: HTTP {status}{message}"));
        }
        Ok(data)
    }
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
