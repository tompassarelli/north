use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::accounts;
use crate::fleet::model::{Account, ProviderName};
use crate::foundation::clock::Timestamp;

const MESSAGES: &str = "https://api.anthropic.com/v1/messages";
const MINIMAL_TURN: &str = r#"{"model":"claude-haiku-5-5","max_tokens":1,"system":"You are Claude Code, Anthropic's official CLI for Claude.","messages":[{"role":"user","content":"."}]}"#;
const SOURCE: &str = "Messages response anthropic-ratelimit-unified headers on the account's setup token";
const WINDOWS: [(&str, &str, u32); 2] = [("5h", "session", 300), ("7d", "weekly_all", 10080)];

pub fn token_accounts(policy: &Path, home: &Path) -> Vec<Account> {
    let profiles = accounts::declared(policy, home);
    profiles
        .tokens
        .iter()
        .filter(|(_, file)| accounts::readable_token(file))
        .filter_map(|(label, file)| {
            let (_, account_home) = profiles.homes.iter().find(|(home_label, _)| home_label == label)?;
            Some(Account { provider: ProviderName::new("claude"), label: label.clone(), home: account_home.clone(), token_file: Some(file.clone()) })
        })
        .collect()
}

pub fn usage_rows(account: &Account, at: f64) -> Result<Vec<Value>, String> {
    let file = account.token_file.as_deref().ok_or("account has no token file")?;
    rows_from(&account.label, &fetch_headers(file)?, at)
}

fn fetch_headers(token_file: &Path) -> Result<String, String> {
    let token = std::fs::read_to_string(token_file).map_err(|error| format!("token file: {error}"))?;
    let token = token.trim();
    let config = curl_config(token)?;
    let mut child = Command::new("curl")
        .args(["--silent", "--show-error", "--max-time", "30", "--config", "-", "--request", "POST", "--dump-header", "-", "--output", "/dev/null", "--data-binary", MINIMAL_TURN, MESSAGES])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("curl: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes()).map_err(|error| format!("curl: {error}"))?;
    }
    let output = child.wait_with_output().map_err(|error| format!("curl: {error}"))?;
    if !output.status.success() {
        return Err(format!("curl exit {}: {}", output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stderr).replace(token, "[token]").trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn curl_config(token: &str) -> Result<String, String> {
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\') {
        return Err("token file does not hold a single-line token".into());
    }
    Ok([format!("Authorization: Bearer {token}"), "anthropic-version: 2023-06-01".into(), "anthropic-beta: oauth-2025-04-20".into(), "content-type: application/json".into()]
        .into_iter()
        .map(|text| format!("header = \"{text}\"\n"))
        .collect())
}

fn rows_from(label: &str, headers: &str, at: f64) -> Result<Vec<Value>, String> {
    let header = |name: &str| {
        headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim().eq_ignore_ascii_case(&format!("anthropic-ratelimit-unified-{name}")).then(|| value.trim())
        })
    };
    let ts = Timestamp::from_unix_seconds_f64(at).iso();
    let rows: Vec<Value> = WINDOWS
        .iter()
        .filter_map(|(key, name, minutes)| {
            let used = header(&format!("{key}-utilization"))?.parse::<f64>().ok()?;
            let reset = header(&format!("{key}-reset")).and_then(|seconds| seconds.parse::<f64>().ok());
            Some(json!({"provider": "claude", "account": label, "source": SOURCE, "ts": ts, "kind": "window", "name": name,
                "used_pct": (used * 100.0).round(), "window_min": minutes,
                "resets_at": reset.map(|seconds| Timestamp::from_unix_seconds_f64(seconds).iso()), "amount": null}))
        })
        .collect();
    if rows.is_empty() { Err("response carried no anthropic-ratelimit-unified utilization headers".into()) } else { Ok(rows) }
}

#[cfg(test)]
#[path = "token_quota_tests.rs"]
mod tests;
