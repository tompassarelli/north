//! Writes `$XDG_STATE_HOME/agents/costs/digitalocean.json`: the current billing month's usage, balance and credit, read
//! from the DigitalOcean balance endpoint (read-only) with the token from south:secrets/digitalocean.yaml.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;
use crate::usage::{profiles, py};

const API: &str = "https://api.digitalocean.com/v2/customers/my/balance";
const FALLBACK_KEY: &str = "/var/lib/sops-nix/key.txt";
const LIMIT: Duration = Duration::from_secs(20);

pub(super) fn refresh() {
    if let Err(error) = write(Timestamp::now()) {
        eprintln!("billing_digitalocean: {error}");
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn write(now: Timestamp) -> Result<(), String> {
    let balance = fetch(&token()?)?;
    let out = env_path("XDG_STATE_HOME").unwrap_or_else(|| profiles::home().join(".local/state")).join("agents/costs/digitalocean.json");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let pending = out.with_extension("tmp");
    std::fs::write(&pending, format!("{}\n", snapshot(&balance, now)?)).and_then(|()| std::fs::rename(&pending, &out)).map_err(|error| error.to_string())
}

fn token() -> Result<String, String> {
    let secret = env_path("SOUTH_SECRETS").unwrap_or_else(|| profiles::home().join("code/south/main/secrets")).join("digitalocean.yaml");
    let key = std::env::var("SOPS_AGE_KEY_FILE").ok().filter(|key| !key.is_empty()).unwrap_or_else(|| FALLBACK_KEY.to_owned());
    let readable = std::fs::File::open(&key).is_ok();
    let sops = ["sops".to_owned(), "-d".into(), "--extract".into(), "[\"token\"]".into(), secret.display().to_string()];
    let mut command = if readable {
        let mut command = Command::new(&sops[0]);
        command.args(&sops[1..]);
        command
    } else {
        let mut command = Command::new("sudo");
        command.args(["-n", "env"]).arg(format!("SOPS_AGE_KEY_FILE={key}")).args(&sops);
        command
    };
    command.env("SOPS_AGE_KEY_FILE", &key);
    let output = crate::platform::process::run_command_within(command, None, LIMIT).filter(|output| output.status.success());
    let token = output.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|token| !token.is_empty());
    token.ok_or_else(|| format!("could not decrypt {}", secret.file_name().and_then(|name| name.to_str()).unwrap_or_default()))
}

fn fetch(token: &str) -> Result<Value, String> {
    if !token.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\') {
        return Err("token is not a single-line token".into());
    }
    let mut child = Command::new("curl")
        .args(["--silent", "--show-error", "--fail", "--max-time", "20", "--config", "-", API])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("curl: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(format!("header = \"Authorization: Bearer {token}\"\n").as_bytes()).map_err(|error| format!("curl: {error}"))?;
    }
    let output = child.wait_with_output().map_err(|error| format!("curl: {error}"))?;
    if !output.status.success() {
        return Err(format!("curl exit {}: {}", output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stderr).replace(token, "[token]").trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("unreadable balance: {error}"))
}

fn amount(balance: &Value, key: &str) -> Result<f64, String> {
    match &balance[key] {
        Value::String(text) => text.trim().parse().map_err(|_| format!("{key} is not a number")),
        other => other.as_f64().ok_or_else(|| format!("{key} is not a number")),
    }
}

fn snapshot(balance: &Value, now: Timestamp) -> Result<Value, String> {
    let moment = py::civil(now);
    let (year, month) = (moment.year(), u8::from(moment.month()));
    let (end_year, end_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let boundary = |year: i32, month: u8| format!("{year:04}-{month:02}-01T00:00:00+00:00");
    let account = amount(balance, "account_balance")?;
    Ok(json!({
        "service": "digitalocean", "period_start": boundary(year, month), "period_end": boundary(end_year, end_month),
        "usd_spent_period": amount(balance, "month_to_date_usage")?, "usd_balance": amount(balance, "month_to_date_balance")?,
        "credits_left": (-account).max(0.0), "read_at": py::isoformat(now).replace('Z', "+00:00"), "source": API,
    }))
}
