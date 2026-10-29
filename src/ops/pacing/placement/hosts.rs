use super::super::storage::load;
use super::super::{process, storage, target};
use super::{local, moment, operator, stamp};
use crate::capacity::saturation::{self, Saturation};
use serde_json::{Value, json};
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
/// Live resident agent CLIs on host `name`, asked through ssh `destination` unless it is this host;
/// None when unreachable.
pub(in crate::ops::pacing) fn resident(name: &str, destination: &str) -> Option<i64> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        if fixture[name].is_object() {
            return fixture[name]["online"]
                .as_bool()
                .unwrap_or(false)
                .then(|| fixture[name]["live"].as_i64().unwrap_or(0));
        }
    }
    if name == local() {
        return live_count(&crate::ops::resident_cap::status());
    }
    let result = process::run(
        "ssh",
        &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, ".local/state/north/current/tree/bin/north", "resident-cap", "status"],
        None,
        20.0,
    );
    result.ok().filter(|r| r.code == 0).and_then(|r| live_count(&r.out))
}

const SATURATION_CACHE_SECONDS: f64 = 60.0;

pub(in crate::ops::pacing) fn here_saturation() -> Option<String> {
    let name = local();
    saturation(&name, "")?.refusal(&name)
}

impl super::gate::Decision {
    pub(crate) fn hold_here(&mut self) {
        let (None, super::gate::Target::Account(_), Some(why)) = (&self.machine, &self.target, &self.here_saturated) else { return };
        let why = why.clone();
        self.machine_lines.push(why.clone());
        self.target = super::gate::Target::Held(why);
    }
}

impl super::gate::Machine {
    pub(in crate::ops::pacing) fn saturation_refusal(&self) -> Option<String> {
        self.saturation?.refusal(&self.name)
    }
}

/// The host's saturation reading, from the PLACEMENT_HOSTS fixture alone when that is set, else the cache when under a
/// minute old, else a fresh probe here or over ssh `destination`; None when unreadable.
pub(in crate::ops::pacing) fn saturation(name: &str, destination: &str) -> Option<Saturation> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        let reading = &fixture[name]["saturation"];
        return Some(Saturation { queued: reading["queued"].as_u64()?.try_into().ok()?, cores: reading["cores"].as_f64()?, load5: reading["load5"].as_f64()?, disk_free_gib: reading["disk_free_gib"].as_f64()? });
    }
    let cache = storage::directory().join("saturation").join(format!("{name}.json"));
    let now = storage::now();
    let cached = load(&cache);
    if cached["at"].as_f64().is_some_and(|at| (0.0..SATURATION_CACHE_SECONDS).contains(&(now - at))) {
        return serde_json::from_value(cached["reading"].clone()).ok();
    }
    let reading = probe(name, destination)?;
    let _ = std::fs::create_dir_all(cache.parent()?).and_then(|()| std::fs::write(&cache, json!({"at": now, "reading": reading}).to_string()));
    Some(reading)
}

fn probe(name: &str, destination: &str) -> Option<Saturation> {
    let output = if name == local() {
        process::run("sh", &["-c", saturation::PROBE], None, 20.0)
    } else {
        let command = format!("sh -c '{}'", saturation::PROBE);
        process::run("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, &command], None, 20.0)
    };
    output.ok().filter(|r| r.code == 0).and_then(|r| saturation::parse(&r.out))
}

pub(in crate::ops::pacing) fn signed_in(name: &str, destination: &str) -> Vec<String> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        if fixture[name].is_object() {
            return fixture[name]["accounts"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        }
    }
    let listed = process::run("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north", "fleet", "accounts"], None, 20.0);
    listed.ok().filter(|r| r.code == 0).map(|r| claude_labels(&r.out)).unwrap_or_default()
}

fn claude_labels(listing: &str) -> Vec<String> {
    listing.lines().filter_map(|line| line.strip_prefix("claude\t")).map(|label| label.trim().to_owned()).collect()
}

/// The live count from `north resident-cap status`, which prints `HOST: LIVE/CAP resident agent CLIs`.
fn live_count(status: &str) -> Option<i64> {
    let (_, rest) = status.split_once(':')?;
    rest.split_whitespace().next()?.split('/').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_live_count_is_read_from_the_resident_cap_status_line() {
        assert_eq!(super::live_count("far: 3/24 resident agent CLIs\n"), Some(3));
        assert_eq!(super::live_count("far: no cap\n"), None);
    }

    #[test]
    fn a_hosts_roster_is_read_from_its_fleet_accounts_listing() {
        assert_eq!(super::claude_labels("claude\ta\nclaude\tc\ncodex\tpooled\n"), ["a", "c"]);
    }

    const NOW: f64 = 1_791_896_400.0;
    const DAY: f64 = 86_400.0;

    #[test]
    fn the_credit_line_reads_unread_spent_or_what_is_left() {
        let unread = || (true, "cloud credit: unread, expires in 22 days".to_owned());
        assert_eq!(super::credit_standing(None, None, 250.0, 22, NOW), unread());
        assert_eq!(
            super::credit_standing(Some(250.0), Some(NOW - DAY), 250.0, 22, NOW),
            (false, "cloud credit: spent (read 2026-10-12), expires in 22 days".to_owned())
        );
        assert_eq!(
            super::credit_standing(Some(100.0), Some(NOW - DAY), 250.0, 22, NOW),
            (true, "cloud credit: $150.00 left (read 2026-10-12), expires in 22 days".to_owned())
        );
        assert_eq!(
            super::credit_standing(Some(100.0), Some(NOW - 8.0 * DAY), 250.0, 22, NOW),
            unread()
        );
    }
}
