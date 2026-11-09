use std::process::Command;
use std::time::Duration;

use north_base::platform::broker_wire::{Request, Response, header};
use north_base::platform::process;

use crate::credentials::{CLOUDFLARE_FILE, DIGITALOCEAN_FILE, HETZNER_FILE, Source};
use crate::policy::Rules;

const FORWARD_LIMIT: Duration = Duration::from_secs(60);
const REQUEST_HEADERS: [&str; 5] = ["accept", "content-type", "if-none-match", "user-agent", "x-auth-email"];
const RESPONSE_HEADERS: [&str; 6] = ["content-type", "etag", "link", "retry-after", "ratelimit-remaining", "ratelimit-reset"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Auth {
    Bearer,
    KeyHeader(&'static str),
}

#[derive(Clone, Debug)]
pub struct Provider {
    pub name: &'static str,
    pub upstream: String,
    file: &'static str,
    field: &'static str,
    auth: Auth,
}

pub fn providers() -> Vec<Provider> {
    let make = |name, upstream: &str, file, field, auth| Provider { name, upstream: upstream.to_owned(), file, field, auth };
    vec![
        make("digitalocean", "https://api.digitalocean.com", DIGITALOCEAN_FILE, "token", Auth::Bearer),
        make("hcloud", "https://api.hetzner.cloud", HETZNER_FILE, "hcloud_token", Auth::Bearer),
        make("cloudflare", "https://api.cloudflare.com", CLOUDFLARE_FILE, "cloudflare-global-api-key", Auth::KeyHeader("X-Auth-Key")),
    ]
}

pub fn split(target: &str) -> Option<(&str, &str, &str)> {
    let rest = target.strip_prefix('/')?;
    let (provider, tail) = rest.split_once('/').map_or((rest, ""), |(provider, tail)| (provider, tail));
    let (path, query) = tail.split_once('?').map_or((tail, ""), |(path, query)| (path, query));
    Some((provider, path, query))
}

impl Provider {
    pub fn forward(&self, source: &Source, rules: &Rules, request: &Request, path: &str, query: &str) -> Response {
        let upstream_path = format!("/{path}");
        if !rules.allows(&request.method, &upstream_path) {
            return Response::refusal(403, &format!("{} {upstream_path} is outside the {} allowlist", request.method, self.name));
        }
        let secret = match source.fields(self.file).and_then(|fields| fields.get(self.field).cloned().filter(|secret| !secret.is_empty()).ok_or_else(|| format!("{} holds no {}", self.file, self.field))) {
            Ok(secret) => secret,
            Err(why) => return Response::refusal(502, &why),
        };
        match self.send(&secret, request, &upstream_path, query) {
            Ok(response) => scrub(response, &secret),
            Err(why) => Response::refusal(502, &why),
        }
    }

    fn send(&self, secret: &str, request: &Request, path: &str, query: &str) -> Result<Response, String> {
        let url = if query.is_empty() { format!("{}{path}", self.upstream) } else { format!("{}{path}?{query}", self.upstream) };
        let mut config = String::new();
        for (name, value) in &request.headers {
            if REQUEST_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                config.push_str(&format!("header = {}\n", quoted(&format!("{name}: {value}"))?));
            }
        }
        let credential = match self.auth {
            Auth::Bearer => format!("Authorization: Bearer {secret}"),
            Auth::KeyHeader(name) => format!("{name}: {secret}"),
        };
        config.push_str(&format!("header = {}\n", quoted(&credential)?));
        if !request.body.is_empty() {
            let body = std::str::from_utf8(&request.body).map_err(|_| "the request body is not text".to_owned())?;
            config.push_str(&format!("data-binary = {}\n", quoted(body)?));
        }
        let mut command = Command::new("curl");
        command.args(["--silent", "--show-error", "--include", "--max-time", "55", "--request", &request.method, "--config", "-", &url]);
        let output = process::run_command_within(command, Some(config.as_bytes()), FORWARD_LIMIT).filter(|output| output.status.success()).ok_or_else(|| format!("could not reach {}", self.upstream))?;
        parse_response(&output.stdout)
    }
}

fn quoted(text: &str) -> Result<String, String> {
    if text.bytes().any(|byte| byte == 0) {
        return Err("a request header or body holds a NUL".to_owned());
    }
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    Ok(out)
}

fn parse_response(raw: &[u8]) -> Result<Response, String> {
    let mut rest = raw;
    loop {
        let end = rest.windows(4).position(|window| window == b"\r\n\r\n").ok_or("the upstream answer has no header block")?;
        let head = String::from_utf8_lossy(&rest[..end]).into_owned();
        let body = &rest[end + 4..];
        let mut lines = head.lines();
        let status: u16 = lines.next().and_then(|line| line.split_whitespace().nth(1)).and_then(|code| code.parse().ok()).ok_or("the upstream status line is unreadable")?;
        if (100..200).contains(&status) {
            rest = body;
            continue;
        }
        let headers: Vec<(String, String)> = lines.filter_map(|line| line.split_once(':')).map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned())).filter(|(name, _)| RESPONSE_HEADERS.contains(&name.to_ascii_lowercase().as_str())).collect();
        return Ok(Response { status, headers, body: body.to_vec() });
    }
}

fn scrub(mut response: Response, secret: &str) -> Response {
    let body = String::from_utf8_lossy(&response.body);
    if body.contains(secret) {
        response.body = body.replace(secret, "placeholder").into_bytes();
    }
    if header(&response.headers, "content-type").is_none() {
        response.headers.push(("Content-Type".to_owned(), "application/octet-stream".to_owned()));
    }
    response
}
