//! Writes `$XDG_STATE_HOME/agents/costs/digitalocean.json`: the current billing month's usage, balance and credit, read
//! from the DigitalOcean balance endpoint (read-only) through the credential broker's DigitalOcean proxy, which holds the token.
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;
use crate::platform::broker_client::Client;
use crate::usage::{profiles, py};

const API: &str = "https://api.digitalocean.com/v2/customers/my/balance";

pub(super) fn refresh() {
    if let Err(error) = write(Timestamp::now()) {
        eprintln!("billing_digitalocean: {error}");
    }
}

fn write(now: Timestamp) -> Result<(), String> {
    let balance = balance()?;
    let out = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| profiles::home().join(".local/state"), PathBuf::from).join("agents/costs/digitalocean.json");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let pending = out.with_extension("tmp");
    std::fs::write(&pending, format!("{}\n", snapshot(&balance, now)?)).and_then(|()| std::fs::rename(&pending, &out)).map_err(|error| error.to_string())
}

fn balance() -> Result<Value, String> {
    Client::discover().call("GET", "/digitalocean/v2/customers/my/balance", None)
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
