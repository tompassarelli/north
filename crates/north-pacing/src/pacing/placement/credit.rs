use super::super::storage::{self, load};
use super::{moment, stamp};
use north_strategy_types::placement::Credit;
use std::path::Path;
use toml_edit::{DocumentMut, Item};

pub(in crate::pacing) fn credit(cfg: &DocumentMut, at: f64) -> (bool, String) {
    let name = credit_name(cfg);
    credit_verdict(&credit_case(&name), &name, at)
}

pub(in crate::pacing) fn credit_name(cfg: &DocumentMut) -> String {
    cfg.get("placement").and_then(|p| p.get("cloud_credit")).and_then(Item::as_str).unwrap_or_default().to_owned()
}

/// The `costs.toml` service `name` names and its spend readout, as placement records them.
pub(in crate::pacing) fn credit_case(name: &str) -> Credit {
    let Some(service) = credit_service(name) else { return Credit::default() };
    let path = storage::directory().join("costs").join(format!("{name}.json"));
    let readout = load(&path);
    Credit {
        configured: true,
        expires: credit_expiry(&service),
        initial: service.get("initial").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64))).unwrap_or(0.0),
        spent: readout["usd_spent_period"].as_f64(),
        read_at: moment(&readout["read_at"]).or_else(|| modified_at(&path)),
    }
}

pub(in crate::pacing) fn credit_verdict(credit: &Credit, name: &str, at: f64) -> (bool, String) {
    if !credit.configured {
        let detail = if name.is_empty() { "no [placement] cloud_credit" } else { name };
        return (false, format!("cloud credit: not configured ({detail})"));
    }
    let Some(expiry) = credit.expires else {
        return (false, format!("cloud credit: not configured ({name})"));
    };
    if expiry <= at {
        return (false, format!("cloud credit: expired {}Z", &stamp(expiry)[..16]));
    }
    let days = ((expiry - at) / 86400.0) as i64;
    credit_standing(credit.spent, credit.read_at, credit.initial, days, at)
}
fn credit_service(name: &str) -> Option<toml_edit::Table> {
    let document = std::fs::read_to_string(crate::platform::orchestration::policy_file("costs.toml"))
        .ok()
        .and_then(|s| s.parse::<DocumentMut>().ok())?;
    document
        .get("service")
        .and_then(Item::as_array_of_tables)
        .and_then(|ss| {
            ss.iter()
                .find(|s| s.get("id").and_then(Item::as_str) == Some(name))
        })
        .cloned()
}
fn credit_expiry(service: &toml_edit::Table) -> Option<f64> {
    service
        .get("expires")
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_datetime().map(ToString::to_string))
        })
        .and_then(|s| storage::seconds(&s))
}
fn modified_at(path: &Path) -> Option<f64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs_f64())
}
fn credit_standing(
    spent: Option<f64>,
    read: Option<f64>,
    initial: f64,
    days: i64,
    at: f64,
) -> (bool, String) {
    let left = format!("expires in {days} days");
    let date = read.map(|t| stamp(t)[..10].to_owned()).unwrap_or_default();
    if spent.is_some_and(|n| n >= initial) {
        return (false, format!("cloud credit: spent (read {date}), {left}"));
    }
    if spent.is_none() || read.is_none_or(|t| at - t > 7.0 * 86400.0) {
        return (true, format!("cloud credit: unread, {left}"));
    }
    (
        true,
        format!(
            "cloud credit: ${:.2} left (read {date}), {left}",
            initial - spent.unwrap()
        ),
    )
}
