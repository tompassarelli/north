//! One cheap Messages request per API organization, used to re-judge an exhausted mark: one token from the
//! smallest model, so a passing answer costs almost nothing and an organization the provider still refuses
//! answers with the refusal body the mark keeps.
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const MESSAGES: &str = "https://api.anthropic.com/v1/messages";
const MINIMAL_TURN: &str =
    r#"{"model":"claude-haiku-5-5","max_tokens":1,"messages":[{"role":"user","content":"."}]}"#;
const ATTEMPT: Duration = Duration::from_secs(8);

/// What a probe's HTTP call answered: whether the organization accepted the turn, its status, and the body.
pub struct Answer {
    pub passed: bool,
    pub status: u16,
    pub body: String,
}

/// Probes the organization whose Messages API key is `key`: a passing answer is `Ok`, a refusal is `Err` with the
/// provider's body.
pub fn probe(key: &Path) -> Result<(), String> {
    let answer = probe_at(MESSAGES, ATTEMPT, key)?;
    if answer.passed {
        Ok(())
    } else {
        Err(answer.body)
    }
}

/// The probe `probe` makes, against `endpoint` within `limit`, for tests that answer it locally.
pub fn probe_at(endpoint: &str, limit: Duration, key: &Path) -> Result<Answer, String> {
    let key = std::fs::read_to_string(key).map_err(|error| format!("key file: {error}"))?;
    let config = curl_config(key.trim())?;
    let limit = limit.as_secs_f64().to_string();
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--connect-timeout",
            &limit,
            "--max-time",
            &limit,
            "--config",
            "-",
            "--request",
            "POST",
            "--header",
            "Expect:",
            "--write-out",
            r"\n%{http_code}",
            "--data-binary",
            MINIMAL_TURN,
            endpoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("curl: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(config.as_bytes())
            .map_err(|error| format!("curl: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "curl exit {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (body, status) = text.rsplit_once('\n').ok_or("curl answered no status")?;
    let status: u16 = status
        .trim()
        .parse()
        .map_err(|_| format!("curl answered a status of {status:?}"))?;
    Ok(Answer {
        passed: (200..300).contains(&status),
        status,
        body: body.to_owned(),
    })
}

pub(super) fn curl_config(key: &str) -> Result<String, String> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\')
    {
        return Err("key file does not hold a single-line key".into());
    }
    Ok([
        format!("x-api-key: {key}"),
        "anthropic-version: 2023-06-01".to_owned(),
        "content-type: application/json".to_owned(),
    ]
    .into_iter()
    .map(|text| format!("header = \"{text}\"\n"))
    .collect())
}

#[cfg(test)]
mod tests;
