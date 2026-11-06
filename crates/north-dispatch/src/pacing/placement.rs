use super::{sampling, storage, target};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use toml_edit::Item;

pub mod case;
pub mod gate;
mod hosts;
pub use hosts::{Pass, pass};
mod inputs;
pub mod needs;
mod reach;
mod repository;
pub mod room;
use hosts::{credit, laptop, resident, saturation};
pub use inputs::{cloud_credit, config, laptop_state, now, pace_directory, pace_state, resident_clis};

pub(super) fn field<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|line| {
            line.trim()
                .split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim())
        })
        .unwrap_or_default()
}
fn facets(text: &str) -> (String, bool, BTreeSet<String>) {
    let needs: BTreeSet<_> = text
        .lines()
        .filter_map(|line| line.trim().split_once(':'))
        .filter(|(k, _)| k.eq_ignore_ascii_case("Needs"))
        .map(|(_, v)| v.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
        .split(|c: char| c.is_whitespace() || c == ',' || c == '|')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let native = field(text, "Verify").split_whitespace().next() == Some("native")
        || needs.contains("native")
        || needs.contains("sign-ins");
    (field(text, "Item").into(), native, needs)
}
fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
pub fn serving_host(needs: &str) -> Option<String> {
    gate::serving(&sampling::config().ok()?, needs, &local())
}
pub fn local_warcraft_clients() -> Option<u32> {
    gate::warcraft_clients(&sampling::config().ok()?, &local())
}
pub fn local_bar1_margin_mib() -> Option<u32> {
    gate::bar1_margin_mib(&sampling::config().ok()?, &local())
}
pub fn local_lease_builds() -> bool {
    sampling::config().ok().and_then(|cfg| cfg.get("hosts")?.get(local())?.get("lease_builds")?.as_bool()).unwrap_or(false) || crate::capacity::mode::leases_own_builds()
}
pub fn machine(name: &str) -> Option<crate::fleet::model::Machine> {
    let machine = gate::machines(&sampling::config().ok()?).into_iter().find(|machine| machine.name == name)?;
    Some(crate::fleet::model::Machine { name: machine.name, ssh: machine.ssh })
}
pub fn saturation_refusal(name: &str) -> Result<Option<String>, String> {
    Ok(saturation_reading(name)?.refusal(name))
}
/// The host's memory, root disk and admission backlog, read here or over its ssh destination.
pub fn saturation_reading(name: &str) -> Result<crate::capacity::saturation::Saturation, String> {
    let destination = if name == local() { String::new() } else { machine(name).map(|machine| machine.ssh).ok_or_else(|| format!("no [hosts.{name}] ssh destination to read its saturation"))? };
    saturation(name, &destination).ok_or_else(|| format!("{name} saturation unreadable"))
}
fn local() -> String {
    crate::platform::host::name()
}
fn operator() -> PathBuf {
    std::env::var_os("AGENTS_OPERATOR_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::platform::orchestration::path()
                .parent()
                .unwrap_or(Path::new("."))
                .to_owned()
        })
}
fn stamp(at: f64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds_f64(at)
        .iso_micros_utc_offset()
        .replace(".000000", "")
        .replace("+00:00", "Z")
}
fn moment(value: &Value) -> Option<f64> {
    value.as_str().and_then(target::seconds)
}
pub(super) fn cli(args: &[String], fleet: super::FleetFacts) -> Result<(), String> {
    let cfg = sampling::config()?;
    let at = std::env::var("PLACEMENT_NOW")
        .ok()
        .and_then(|s| target::seconds(&s))
        .unwrap_or_else(storage::now);
    if args == ["credit"] {
        println!("{}", credit(&cfg, at).1);
        return Ok(());
    }
    if args.first().is_none_or(|s| s != "place") || args.len() < 2 {
        return Err("north place place BRIEF... | credit".into());
    }
    let fleet_load = (fleet.machine_load)().map_err(|error| error.to_string())?;
    let roster = (fleet.roster)().map_err(|error| error.to_string())?;
    for path in &args[1..] {
        let text = if path == "-" {
            use std::io::Read;
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            text
        } else {
            std::fs::read_to_string(path).map_err(|e| e.to_string())?
        };
        let text = gate::anchored(&text, std::env::current_dir().ok().as_deref(), None);
        let decision = gate::live(&text, &fleet_load, &roster, None)?;
        for line in decision.verdicts.iter().map(|v| &v.line).chain(&decision.machine_lines) {
            println!("  {line}");
        }
        let item = field(&text, "Item");
        println!(
            "{}: {}",
            if item.is_empty() { path } else { item },
            gate::summary(&decision)
        );
    }
    Ok(())
}
