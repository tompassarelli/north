use std::io::Write;
use std::process::Command;
use std::time::Duration;

use super::http::{Request, Response, status_code};
use crate::platform::process;

pub const GITHUB_API: &str = "https://api.github.com";
const API_HOST: &str = "api.github.com";
const LIMIT: Duration = Duration::from_secs(300);
const NOT_FORWARDED: &[&str] = &["host", "connection", "accept-encoding", "content-length", "transfer-encoding", "expect", "if-none-match", "if-modified-since"];

pub struct Upstream {
    api: String,
}

impl Upstream {
    pub fn new(api: &str) -> Self {
        Self { api: api.trim_end_matches('/').to_owned() }
    }

    fn url(&self, request: &Request) -> String {
        match request.header("host").filter(|host| *host != API_HOST) {
            Some(host) => format!("https://{host}{}", request.target),
            None => format!("{}{}", self.api, request.target),
        }
    }

    pub fn send(&self, request: &Request, etag: Option<&str>) -> Response {
        match self.curl(request, etag) {
            Ok(response) => response,
            Err(reason) => Response::json(502, Vec::new(), &format!("north gh-broker could not reach GitHub: {reason}")),
        }
    }

    fn curl(&self, request: &Request, etag: Option<&str>) -> Result<Response, String> {
        let mut command = Command::new("curl");
        command.args(["--silent", "--show-error", "--include", "--max-time", "290", "--config", "-"]);
        let body = attach_body(&mut command, request)?;
        command.arg(self.url(request));
        let config = config(request, etag);
        let output = process::run_command_within(command, Some(config.as_bytes()), LIMIT).ok_or("curl did not finish")?;
        drop(body);
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        parse(&output.stdout).ok_or_else(|| "curl printed no HTTP response".to_owned())
    }
}

fn attach_body(command: &mut Command, request: &Request) -> Result<Option<tempfile::NamedTempFile>, String> {
    match request.method.as_str() {
        "GET" => Ok(None),
        "HEAD" => {
            command.arg("--head");
            Ok(None)
        }
        method => {
            let mut file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
            file.write_all(&request.body).map_err(|error| error.to_string())?;
            command.args(["--request", method, "--data-binary"]).arg(format!("@{}", file.path().display()));
            Ok(Some(file))
        }
    }
}

fn config(request: &Request, etag: Option<&str>) -> String {
    let mut lines = Vec::new();
    let mut line = |name: &str, value: &str| lines.push(format!("header = \"{}\"", quote(&format!("{name}: {value}"))));
    for (name, value) in request.headers.iter().filter(|(name, _)| !NOT_FORWARDED.contains(&name.to_ascii_lowercase().as_str())) {
        line(name, value);
    }
    if let Some(etag) = etag {
        line("If-None-Match", etag);
    }
    if request.header("content-type").is_none() {
        lines.push("header = \"Content-Type:\"".to_owned());
    }
    lines.push("header = \"Expect:\"".to_owned());
    lines.join("\n") + "\n"
}

fn quote(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse(output: &[u8]) -> Option<Response> {
    let mut rest = output;
    loop {
        let end = rest.windows(4).position(|window| window == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&rest[..end]);
        rest = &rest[end + 4..];
        let mut lines = head.split("\r\n");
        let status = status_code(lines.next()?)?;
        if (100..200).contains(&status) {
            continue;
        }
        let headers = lines.filter_map(|line| line.split_once(':')).map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()));
        let headers = headers.filter(|(name, _)| !["content-length", "transfer-encoding", "connection", "content-encoding", "x-north-broker"].contains(&name.to_ascii_lowercase().as_str()));
        return Some(Response { status, headers: headers.collect(), body: rest.to_vec() });
    }
}
