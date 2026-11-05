#[cfg(test)]
mod tests;

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use crate::foundation::clock::{Timestamp, parse_python_date};

const TIMEOUT: Duration = Duration::from_secs(90);
const FAILED: &str = "usage-web-read: Chrome read failed or returned invalid data; previous cache kept";
const TIMED_OUT: &str = "usage-web-read: Chrome read timed out after 90 seconds; previous cache kept";
const SIGN_IN: &str = "usage-web-read: NEEDS-SIGNIN: claude.ai; previous cache kept";
const CACHED: &str = "usage-web-read: Claude reset availability cached";

pub(super) enum Chrome {
    Reply(String),
    Failed,
    TimedOut,
}

pub fn run(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north usage-web-read");
        return 2;
    }
    let home = PathBuf::from(env::var("HOME").unwrap_or_default());
    let set = |name: &str| env::var(name).ok().filter(|value| !value.is_empty());
    let state = set("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
    let operator = set("AGENTS_OPERATOR_DIR").map_or_else(|| home.join("code/south/main/agents"), PathBuf::from);
    let orchestration = set("AGENTS_ORCHESTRATION").map_or_else(|| operator.join("orchestration.toml"), PathBuf::from);
    let program = set("USAGE_WEB_READ_CHROME").unwrap_or_else(|| "automation-chrome".to_owned());
    match read(&state.join("agents/usage/claude-web.json"), &orchestration, run_chrome(&program, TIMEOUT)) {
        Ok(()) => {
            println!("{CACHED}");
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

pub(super) fn run_chrome(program: &str, timeout: Duration) -> Chrome {
    let spawned = Command::new(program).arg("tool").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = spawned else { return Chrome::Failed };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(json!({"action": "usage"}).to_string().as_bytes());
    }
    let mut stdout = child.stdout.take();
    let reader = thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(stdout) = stdout.as_mut() {
            let _ = stdout.read_to_end(&mut text);
        }
        text
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Chrome::TimedOut;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return Chrome::Failed,
        }
    };
    let text = reader.join().unwrap_or_default();
    if status.success() { Chrome::Reply(String::from_utf8_lossy(&text).into_owned()) } else { Chrome::Failed }
}

fn valid_uuid(text: &str) -> bool {
    let trimmed = text.replace("urn:", "").replace("uuid:", "");
    let digits = trimmed.trim_matches(|c| c == '{' || c == '}').replace('-', "");
    digits.len() == 32 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn read_time(text: &str) -> Option<String> {
    let at = Timestamp::parse_rfc3339(text)?;
    let micros = at.unix_nanos().rem_euclid(1_000_000_000) / 1000;
    Some(if micros == 0 { at.iso() } else { at.iso_micros_utc_offset().replace("+00:00", "Z") })
}

fn label(orchestration: &Path, account: &str) -> Option<Value> {
    let document = fs::read_to_string(orchestration).ok()?.parse::<DocumentMut>().ok()?;
    let found = document.get("accounts")?.get("claude")?.get("labels")?.get(account)?;
    Some(Value::String(found.as_str()?.to_owned()))
}

fn entry(reading: &Value, orchestration: &Path) -> Option<(String, Value)> {
    let uuid = reading.get("account_uuid")?.as_str()?;
    if !valid_uuid(uuid) {
        return None;
    }
    let digest = Sha256::digest(uuid.as_bytes());
    let account: String = digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect();
    let read_at = read_time(reading.get("read_at")?.as_str()?)?;
    let mut resets = Map::new();
    let mut total = Some(0_i64);
    for kind in ["full", "five_hour"] {
        let grant = reading.get("resets")?.get(kind)?;
        let count = grant.as_object()?.get("count")?;
        let counted = match count {
            Value::Null => None,
            other => Some(other.as_i64().filter(|count| *count >= 0)?),
        };
        total = total.zip(counted).map(|(sum, count)| sum + count);
        let expires = grant.get("expires").cloned().unwrap_or(Value::Null);
        if truthy(&expires) && parse_python_date(expires.as_str()?).is_none() {
            return None;
        }
        resets.insert(kind.to_owned(), json!({"count": count, "expires": expires}));
    }
    let windows = reading.get("windows").filter(|windows| windows.is_array())?;
    let resets_text = reading.get("resets_text").filter(|text| text.is_string())?;
    let mut entry = json!({
        "account": account,
        "read_at": read_at,
        "resets_available": total,
        "resets_text": resets_text,
        "windows": windows,
        "resets": resets,
        "source": "claude.ai settings/usage via Chrome",
    });
    if let Some(label) = label(orchestration, &account) {
        entry["account_label"] = label;
    }
    Some((account, entry))
}

pub(super) fn read(cache_path: &Path, orchestration: &Path, chrome: Chrome) -> Result<(), &'static str> {
    let text = match chrome {
        Chrome::Reply(text) => text,
        Chrome::Failed => return Err(FAILED),
        Chrome::TimedOut => return Err(TIMED_OUT),
    };
    let reply: Value = serde_json::from_str(&text).map_err(|_| FAILED)?;
    let reply = reply.as_object().ok_or(FAILED)?;
    if reply.get("error").and_then(Value::as_str).is_some_and(|error| error.starts_with("NEEDS-SIGNIN")) {
        return Err(SIGN_IN);
    }
    let (account, entry) = entry(reply.get("result").ok_or(FAILED)?, orchestration).ok_or(FAILED)?;
    let mut cache = if cache_path.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(cache_path).map_err(|_| FAILED)?).map_err(|_| FAILED)?
    } else {
        json!({"accounts": {}})
    };
    let accounts = cache.as_object_mut().ok_or(FAILED)?.entry("accounts").or_insert_with(|| json!({}));
    accounts.as_object_mut().ok_or(FAILED)?.insert(account, entry);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|_| FAILED)?;
    }
    let temporary = cache_path.with_extension("tmp");
    fs::write(&temporary, format!("{cache}\n")).map_err(|_| FAILED)?;
    fs::rename(temporary, cache_path).map_err(|_| FAILED)
}
