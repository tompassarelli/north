//! The sessions API transport: curl's config and arguments, one JSON request and its paging, the
//! session row and event reads, and the credential lookup a call starts from.
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

const BASE: &str = "https://api.anthropic.com";

/// One call to the sessions API: the method, the path and the optional JSON body, answered with the
/// HTTP status and the response text.
pub(in crate::cli) type Api<'a> =
    dyn FnMut(&str, &str, Option<&str>) -> Result<(u16, String), String> + 'a;

/// A sessions API client carrying the login the cloud sessions belong to.
pub(super) struct Curl {
    pub(super) base: String,
    pub(super) token: String,
    pub(super) org: String,
}

pub(in crate::cli) fn curl_config(token: &str, org: &str) -> String {
    let quote = |text: String| {
        format!(
            "header = \"{}\"\n",
            text.replace('\\', "\\\\").replace('"', "\\\"")
        )
    };
    [
        format!("Authorization: Bearer {token}"),
        "anthropic-version: 2023-06-01".to_owned(),
        "anthropic-beta: ccr-byoc-2025-07-29".to_owned(),
        format!("x-organization-uuid: {org}"),
        "content-type: application/json".to_owned(),
    ]
    .into_iter()
    .map(quote)
    .collect()
}

pub(in crate::cli) fn curl_args(method: &str, url: &str, body: Option<&Path>) -> Vec<String> {
    let mut args = [
        "--silent",
        "--show-error",
        "--max-time",
        "60",
        "--config",
        "-",
        "--request",
        method,
        "--write-out",
        "\n%{http_code}",
    ]
    .map(str::to_owned)
    .to_vec();
    if let Some(body) = body {
        args.extend(["--data-binary".to_owned(), format!("@{}", body.display())]);
    }
    args.push(url.to_owned());
    args
}

impl Curl {
    pub(super) fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        let file = body
            .map(|body| {
                let mut file = tempfile::NamedTempFile::new()?;
                file.write_all(body.as_bytes())?;
                Ok::<_, std::io::Error>(file)
            })
            .transpose()
            .map_err(|error| format!("curl body: {error}"))?;
        let mut child = Command::new("curl")
            .args(curl_args(
                method,
                &format!("{}{path}", self.base),
                file.as_ref().map(|file| file.path()),
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("curl: {error}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(curl_config(&self.token, &self.org).as_bytes())
                .map_err(|error| format!("curl: {error}"))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("curl: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "{method} {path}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let (body, status) = text
            .rsplit_once('\n')
            .ok_or_else(|| format!("{method} {path}: no HTTP status"))?;
        let status = status
            .trim()
            .parse()
            .map_err(|_| format!("{method} {path}: bad HTTP status {status}"))?;
        Ok((status, body.to_owned()))
    }
}

pub(in crate::cli) fn request(
    api: &mut Api,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<Value, String> {
    let (status, text) = api(method, path, body)?;
    if !(200..300).contains(&status) {
        return Err(format!("{method} {path}: HTTP {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|error| format!("{method} {path}: {error}"))
}

pub(super) fn pages(api: &mut Api, path: &str) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let mut after = String::new();
    loop {
        let page = request(api, "GET", &format!("{path}{after}"), None)?;
        items.extend(
            page.get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        match page.get("last_id").and_then(Value::as_str) {
            Some(last) if page.get("has_more") == Some(&Value::Bool(true)) => {
                after = format!("&after_id={last}")
            }
            _ => return Ok(items),
        }
    }
}

pub(in crate::cli) fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub(super) fn row(session: &Value) -> String {
    [
        "id",
        "session_status",
        "environment_kind",
        "updated_at",
        "title",
    ]
    .map(|key| field(session, key).replace(['\t', '\n'], " "))
    .join("\t")
}

pub(in crate::cli) fn events(api: &mut Api, id: &str) -> Result<Vec<Value>, String> {
    pages(
        api,
        &format!("/v1/sessions/{id}/events?beta=true&limit=1000"),
    )
}

pub(in crate::cli) fn save(events: &[Value], out: &Path) -> Result<(), String> {
    let error = |error: std::io::Error| format!("{}: {error}", out.display());
    let mut file = BufWriter::new(File::create(out).map_err(error)?);
    for event in events {
        writeln!(file, "{event}").map_err(error)?;
    }
    file.flush().map_err(error)
}

pub(super) fn credentials() -> Result<Curl, String> {
    let login = crate::dispatch::cloud_login()?;
    let read = |path: PathBuf, pointer: &str| {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let value = serde_json::from_str::<Value>(&text)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("{}: no {pointer}", path.display()))
    };
    Ok(Curl {
        base: BASE.to_owned(),
        token: read(login.credentials, "/claudeAiOauth/accessToken")?,
        org: read(
            login.home.join(".claude.json"),
            "/oauthAccount/organizationUuid",
        )?,
    })
}
