mod config;
mod goals;
mod ledger;
mod live;
mod readout;
mod signin;

use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

use crate::foundation::clock::Timestamp;
use crate::router::policy::json_value;

use super::lines::{ask, plan_lines};
use super::tiers::{Mode, Tier, bands_of, provider, resolve_bands, resolve_mode};
use config::{Config, read_document};
use ledger::Ledger;
use live::Reading;

pub(super) struct Options {
    pub json: bool,
    pub mode: String,
    pub at: Option<PathBuf>,
}

impl Options {
    pub(super) fn parse(arguments: &[String]) -> Option<Self> {
        let mut options = Self { json: false, mode: String::new(), at: None };
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.as_str() {
                "--json" => options.json = true,
                "--mode" => options.mode = rest.next()?.clone(),
                "--at" => options.at = Some(PathBuf::from(rest.next()?)),
                _ => return None,
            }
        }
        Some(options)
    }
}

fn die(message: &str) -> u8 {
    eprintln!("agents plan: {message}");
    2
}

pub(in crate::ops::agents) fn update_status() {
    match Command::new("update-status").output() {
        Ok(done) => {
            eprint!("{}{}", String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => eprintln!("agents plan: cannot run update-status: {error}"),
    }
}

pub(super) fn run(options: &Options, harness: &Path, config_path: &Path) -> u8 {
    match status(options, harness, config_path) {
        Ok(code) => code,
        Err(message) => die(&message),
    }
}
struct Providers {
    usable: Vec<String>,
    dropped: Vec<(String, String)>,
}

fn sign_in(mode: &Mode, quiet: bool) -> Providers {
    let mut found = Providers { usable: Vec::new(), dropped: Vec::new() };
    for name in &mode.providers {
        let (usable, why) = if mode.wanted.contains(name) {
            let (signed_in, how) = signin::check(name);
            (signed_in, format!("not signed in ({how})"))
        } else {
            (false, format!("mode is {}", mode.name))
        };
        if !quiet {
            if usable {
                println!("{name}: signed in");
            } else {
                println!("{name}: dropped, {why}");
            }
        }
        if usable {
            found.usable.push(name.clone());
        } else {
            found.dropped.push((name.clone(), why));
        }
    }
    found
}

struct Live {
    ctx: Value,
    result: Value,
    ledger: Ledger,
    open_error: String,
    ledger_path: PathBuf,
    now: Timestamp,
    used: Vec<Tier>,
}

fn gather(config: &Config, usable: &[String], catalog: &Value, costs: &Value) -> Result<Live, String> {
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    let explicit = env::var_os("THREADS_DB").filter(|path| !path.is_empty());
    let ledger_path = crate::usage::ledger_path(&home, explicit.clone());
    let opened = crate::usage::open_ledger(&home, explicit, &crate::threads::client::refresh_mirrored_snapshot);
    let open_error = opened.as_ref().err().cloned().unwrap_or_default();
    let connection = opened.ok();
    let now = Timestamp::now();
    let readings: Vec<Value> = connection.as_ref().and_then(|db| crate::usage::json_readings(db, now).ok()).unwrap_or_default();
    let ledger = Ledger(connection);
    let rows: Vec<Reading> = readings
        .into_iter()
        .filter_map(|row| serde_json::from_value::<Reading>(row).ok())
        .filter(|row| usable.contains(&row.provider))
        .collect();
    let used: Vec<Tier> = config.tiers.iter().filter(|tier| usable.iter().any(|name| name == provider(&tier.key))).cloned().collect();
    let ctx = live::context(config, catalog, &rows, &ledger, now, &used, costs);
    let result = ask("plan", json!({"ctx": ctx}))?;
    Ok(Live { ctx, result, ledger, open_error, ledger_path, now, used })
}

fn unavailable(result: &Value) -> Vec<String> {
    let capacity = result["capacity"].as_object().cloned().unwrap_or_default();
    capacity.iter().filter(|(_, hit)| !hit.get("fallback").is_some_and(crate::router::ctx::truthy)).map(|(name, _)| name.clone()).collect()
}

fn status(options: &Options, harness: &Path, config_path: &Path) -> Result<u8, String> {
    let config = Config::read(config_path)?;
    let mode = resolve_mode(&config.tiers, config.mode(), &options.mode)?;
    if !options.json {
        println!("mode: {}", mode.name);
    }
    let providers = sign_in(&mode, options.json);
    if providers.usable.is_empty() {
        if options.json {
            println!("{}", json!({"mode": mode.name, "usable": [], "bands": [], "escalation": []}));
        } else {
            println!("bands: none; recommend to Tom (no provider is usable)");
        }
        return Ok(1);
    }
    let catalog = read_document(&harness.join("harness/model-catalog.toml"))?;
    let catalog_value = json_value(catalog.as_item());
    let costs = json_value(read_document(&config_path.with_file_name("costs.toml"))?.as_item());
    let live = gather(&config, &providers.usable, &catalog_value, &costs)?;
    let down = unavailable(&live.result);
    let available: Vec<String> = live.used.iter().filter(|tier| !down.iter().any(|name| name == provider(&tier.key))).map(|tier| tier.key.clone()).collect();
    if options.json {
        print_json(&config, &mode, &providers, &live, &available);
        return Ok(0);
    }
    print_bands(&config, &mode, &providers, &live, &available, &down);
    let mut shown = plan_lines(&live.ctx, &live.result)?;
    shown.extend(readout::promotion_lines(&catalog_value["accounts"], &live.ledger, live.now));
    println!("{}", shown.join("\n"));
    let goals = goals::lines(harness, config_path, &live.ledger_path);
    if !goals.is_empty() {
        println!("{}", goals.join("\n"));
    }
    let measured = readout::measured_runs(&live.ledger, &live.open_error);
    if let Err(error) = &measured {
        println!("measured runs unavailable: {error}");
    }
    readout::catalog(&catalog, &config.tiers, &providers.usable, &mode.providers, &measured)?;
    Ok(0)
}

fn print_json(config: &Config, mode: &Mode, providers: &Providers, live: &Live, available: &[String]) {
    let bands: Vec<Value> =
        resolve_bands(&bands_of(&config.tiers), available).iter().map(|band| json!([whole_number(band.lo), whole_number(band.hi), band.key])).collect();
    let labels: Map<String, Value> = config.tiers.iter().map(|tier| (tier.key.clone(), json!(format!("{} {}", tier.model, tier.effort)))).collect();
    let mut snapshot = json!({"mode": mode.name, "usable": providers.usable, "bands": bands, "escalation": available});
    for key in ["accounts", "routes", "capacity"] {
        snapshot[key] = live.result[key].clone();
    }
    snapshot["projects"] = live.ctx["projects"].clone();
    snapshot["tiers"] = Value::Object(labels);
    println!("{snapshot}");
}

fn print_bands(config: &Config, mode: &Mode, providers: &Providers, live: &Live, available: &[String], down: &[String]) {
    println!("bands (difficulty percentile: start tier):");
    if available.is_empty() {
        let reasons: Vec<String> = mode
            .providers
            .iter()
            .map(|name| match providers.dropped.iter().find(|(known, _)| known == name) {
                Some((_, why)) => format!("{name} dropped, {why}"),
                None => format!("{name} capacity error since {}", super::lines::text(&live.result["capacity"][name]["since"])),
            })
            .collect();
        println!("  none: no provider available ({})", reasons.join("; "));
    }
    let full = bands_of(&config.tiers);
    for line in readout::band_lines(&config.tiers, &full, &resolve_bands(&full, available), down) {
        println!("{line}");
    }
    let labels: Vec<String> = config.tiers.iter().filter(|tier| available.contains(&tier.key)).map(Tier::label).collect();
    println!("escalation: {}, then recommend to Tom", labels.join(", "));
    if live.ctx["accounts"].as_object().is_none_or(|accounts| accounts.is_empty()) {
        println!("accounts: no usage snapshot; run `agents usage --refresh`");
    }
}

fn whole_number(number: f64) -> Value {
    if number.fract() == 0.0 { json!(number as i64) } else { json!(number) }
}
