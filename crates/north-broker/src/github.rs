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
use crate::policy::{Ceiling, Permissions, Scope, rank};

const EXCHANGE_LIMIT: Duration = Duration::from_secs(30);
const INSTALLATION_TOKEN_MARGIN: i64 = 60;

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

    pub fn holds_installation_token(&self) -> bool {
        self.source.path(INSTALLATION_TOKEN_FILE).is_some()
    }

    pub fn mint(&self, app: App, scope: &Scope, ceiling: &Ceiling, valid_for: i64, now: i64) -> Result<Minted, (u16, String)> {
        if app.file == AGENT_APP_FILE && self.holds_installation_token() {
            return self.installation_token(scope, ceiling, valid_for, now);
        }
        self.exchange_app(app, scope, now).map_err(|why| (502, why))
    }

    fn exchange_app(&self, app: App, scope: &Scope, now: i64) -> Result<Minted, String> {
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

    fn installation_token(&self, scope: &Scope, ceiling: &Ceiling, valid_for: i64, now: i64) -> Result<Minted, (u16, String)> {
        let held = self.held_token(now).map_err(|why| (502, why))?;
        held.within_request(scope, valid_for)?;
        held.within_ceiling(ceiling)?;
        Ok(Minted { token: held.token, expires_at: held.expires_at, repositories: Some(held.repositories), permissions: held.permissions })
    }

    fn held_token(&self, now: i64) -> Result<HeldToken, String> {
        let fields = self.source.fields(INSTALLATION_TOKEN_FILE)?;
        let token = fields.get("token").filter(|token| !token.is_empty()).ok_or_else(|| format!("{INSTALLATION_TOKEN_FILE} holds no token"))?;
        let expires_at = fields.get("expires_at").ok_or_else(|| format!("{INSTALLATION_TOKEN_FILE} holds no expires_at"))?;
        let expiry = Timestamp::parse_rfc3339(expires_at).ok_or_else(|| format!("{INSTALLATION_TOKEN_FILE} expires_at is not RFC 3339"))?;
        let left = expiry.unix_seconds() - now;
        if left <= 0 {
            return Err(format!("{INSTALLATION_TOKEN_FILE} expired at {expires_at}"));
        }
        if left < INSTALLATION_TOKEN_MARGIN {
            return Err(format!("{INSTALLATION_TOKEN_FILE} has {left} s left, under the {INSTALLATION_TOKEN_MARGIN} s a token must keep"));
        }
        let (repositories, permissions) = held_scope(&fields)?;
        Ok(HeldToken { token: token.clone(), expires_at: expires_at.clone(), left, repositories, permissions })
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

struct HeldToken {
    token: String,
    expires_at: String,
    left: i64,
    repositories: Vec<String>,
    permissions: Permissions,
}

impl HeldToken {
    fn within_request(&self, scope: &Scope, valid_for: i64) -> Result<(), (u16, String)> {
        if let Some(outside) = scope.repositories.iter().find(|name| !self.repositories.contains(name)) {
            return Err((403, format!("repository {outside} is outside the scope {INSTALLATION_TOKEN_FILE} holds ({})", self.repositories.join(", "))));
        }
        for (name, level) in &scope.permissions {
            if self.permissions.get(name).is_none_or(|most| rank(level) > rank(most)) {
                return Err((403, format!("permission {name}:{level} is outside the scope {INSTALLATION_TOKEN_FILE} holds")));
            }
        }
        if valid_for > self.left {
            return Err((403, format!("valid_for {valid_for} s runs past the {} s {INSTALLATION_TOKEN_FILE} has left", self.left)));
        }
        Ok(())
    }

    fn within_ceiling(&self, ceiling: &Ceiling) -> Result<(), (u16, String)> {
        if let Some(allowed) = &ceiling.repositories
            && let Some(outside) = self.repositories.iter().find(|name| !allowed.contains(name))
        {
            return Err((403, format!("repository {outside} is held in {INSTALLATION_TOKEN_FILE} but outside this caller's ceiling")));
        }
        for (name, level) in &self.permissions {
            if name == "metadata" {
                continue;
            }
            let beyond = match &ceiling.permissions {
                None => name == "workflows",
                Some(most) => most.get(name).is_none_or(|most| rank(level) > rank(most)),
            };
            if beyond {
                return Err((403, format!("permission {name}:{level} is held in {INSTALLATION_TOKEN_FILE} but outside this caller's ceiling")));
            }
        }
        Ok(())
    }
}

fn held_scope(fields: &BTreeMap<String, String>) -> Result<(Vec<String>, Permissions), String> {
    let repositories: Vec<String> = fields.get("repositories").map(|list| list.split(',').map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned).collect()).unwrap_or_default();
    let permissions: Option<Permissions> = fields.get("permissions").map(|list| list.split(',').map(str::trim).filter(|pair| !pair.is_empty()).map(|pair| pair.split_once(':').map(|(name, level)| (name.trim().to_owned(), level.trim().to_owned()))).collect()).unwrap_or_default();
    match (repositories.is_empty(), permissions.filter(|permissions| !permissions.is_empty())) {
        (false, Some(permissions)) => Ok((repositories, permissions)),
        _ => Err(format!("{INSTALLATION_TOKEN_FILE} names no repositories or permissions, so the scope it holds is unknown")),
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
