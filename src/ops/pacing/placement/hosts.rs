use super::super::storage::load;
use super::super::{process, sampling, storage, target};
use super::{local, moment, operator, stamp};
use serde_json::Value;
use std::path::Path;
use toml_edit::{DocumentMut, Item};

pub(in crate::ops::pacing) fn credit(cfg: &DocumentMut, at: f64) -> (bool, String) {
    let name = cfg
        .get("placement")
        .and_then(|p| p.get("cloud_credit"))
        .and_then(Item::as_str)
        .unwrap_or_default();
    let Some(service) = credit_service(name) else {
        let detail = if name.is_empty() {
            "no [placement] cloud_credit"
        } else {
            name
        };
        return (false, format!("cloud credit: not configured ({detail})"));
    };
    let Some(expiry) = credit_expiry(&service) else {
        return (false, format!("cloud credit: not configured ({name})"));
    };
    if expiry <= at {
        return (
            false,
            format!("cloud credit: expired {}Z", &stamp(expiry)[..16]),
        );
    }
    let days = ((expiry - at) / 86400.0) as i64;
    let path = storage::directory()
        .join("costs")
        .join(format!("{name}.json"));
    let readout = load(&path);
    let spent = readout["usd_spent_period"].as_f64();
    let read = moment(&readout["read_at"]).or_else(|| modified_at(&path));
    let initial = service
        .get("initial")
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
        .unwrap_or(0.0);
    credit_standing(spent, read, initial, days, at)
}
fn credit_service(name: &str) -> Option<toml_edit::Table> {
    let document = std::fs::read_to_string(operator().join("costs.toml"))
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
        .and_then(|s| target::seconds(&s))
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
pub(in crate::ops::pacing) fn laptop() -> String {
    if let Ok(path) = std::env::var("LAPTOP_STATE") {
        return load(Path::new(&path))["state"]
            .as_str()
            .unwrap_or("unknown")
            .into();
    }
    process::run("laptop", &["status", "--json"], None, 20.0)
        .ok()
        .filter(|r| r.code == 0)
        .and_then(|r| serde_json::from_str::<Value>(&r.out).ok())
        .and_then(|v| v["state"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}
pub(in crate::ops::pacing) fn host(name: &str) -> (bool, i64) {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        if fixture[name].is_object() {
            return (
                fixture[name]["online"].as_bool().unwrap_or(false),
                fixture[name]["live"].as_i64().unwrap_or(0),
            );
        }
    }
    let root = sampling::root();
    let result = if name == local() {
        process::run(
            "python3",
            &[
                "-I",
                &root.join("harness/lib/resident_cap.py").to_string_lossy(),
                "status",
            ],
            None,
            20.0,
        )
    } else {
        process::run(
            "ssh",
            &[
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                name,
                "python3",
                "-I",
                "code/north/main/harness/lib/resident_cap.py",
                "status",
            ],
            None,
            20.0,
        )
    };
    result
        .ok()
        .filter(|r| r.code == 0)
        .and_then(|r| {
            r.out
                .split_once(':')
                .and_then(|(_, s)| s.split_whitespace().next())
                .and_then(|s| s.parse().ok())
                .map(|n| (true, n))
        })
        .unwrap_or((false, 0))
}
