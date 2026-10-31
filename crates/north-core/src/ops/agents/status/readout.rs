use std::collections::BTreeMap;

use serde_json::Value;
use toml_edit::DocumentMut;

use crate::foundation::clock::Timestamp;
use crate::router::policy::json_value;

use super::super::lines::{text, whole};
use super::super::tiers::{Band, Tier, provider};
use super::ledger::{Ledger, run_tier};

pub(super) type Measured = Result<BTreeMap<String, (i64, i64)>, String>;

fn field(value: &Value, key: &str) -> String {
    value.get(key).map_or("None".to_owned(), text)
}

pub(super) fn measured_runs(ledger: &Ledger, fallback: &str) -> Measured {
    let Some(db) = &ledger.0 else {
        return Err(fallback.to_owned());
    };
    let counts = db
        .prepare("SELECT tier, COUNT(*), SUM(outcome = 'done') FROM runs GROUP BY tier")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<i64>>(2)?)))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .map_err(|error| error.to_string())?;
    Ok(counts.into_iter().map(|(tier, runs, done)| (tier.unwrap_or_default(), (runs, done.unwrap_or(0)))).collect())
}

pub(super) fn band_lines(tiers: &[Tier], full: &[Band], available: &[Band], unavailable: &[String]) -> Vec<String> {
    let find = |key: &str| tiers.iter().find(|tier| tier.key == key);
    let mut lines = Vec::new();
    for band in available {
        let mut took: Vec<(&str, Vec<String>)> = Vec::new();
        for other in full.iter().filter(|other| !available.iter().any(|kept| kept.key == other.key) && other.lo < band.hi && other.hi > band.lo) {
            let why = if unavailable.iter().any(|name| name == provider(&other.key)) { "provider capacity" } else { "provider dropped" };
            let entry = format!("{} {}-{}", other.key, whole(other.lo), whole(other.hi));
            match took.iter_mut().find(|(known, _)| *known == why) {
                Some((_, entries)) => entries.push(entry),
                None => took.push((why, vec![entry])),
            }
        }
        let note = if took.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = took.iter().map(|(why, entries)| format!("{}: {why}", entries.join(", "))).collect();
            format!(" (takes {})", parts.join("; "))
        };
        let tier = find(&band.key);
        let role = tier.and_then(|tier| tier.role.clone()).unwrap_or_else(|| band.key.clone());
        lines.push(format!("  {}-{} {role}: {}{note}", whole(band.lo), whole(band.hi), tier.map(Tier::label).unwrap_or_default()));
    }
    lines
}

pub(super) fn promotion_lines(accounts: &Value, ledger: &Ledger, now: Timestamp) -> Vec<String> {
    let Some(accounts) = accounts.as_object() else {
        return Vec::new();
    };
    let number = |value: &Value, key: &str| value.get(key).and_then(Value::as_f64);
    let full: BTreeMap<&str, f64> = accounts
        .iter()
        .filter_map(|(name, account)| {
            let price = number(account, "monthly_price")?;
            Some((name.as_str(), number(account, "monthly_total").unwrap_or(price * number(account, "account_count").unwrap_or(1.0))))
        })
        .collect();
    let mut lines = Vec::new();
    for (name, account) in accounts {
        let Some(end) = account.get("promo_end").and_then(Value::as_str) else {
            continue;
        };
        let Some(midnight) = Timestamp::parse_rfc3339(&format!("{end}T00:00:00Z")) else {
            continue;
        };
        let left = (midnight.unix_seconds() - now.unix_seconds()).div_euclid(86400);
        if !(0..=7).contains(&left) {
            continue;
        }
        let since = super::ledger::shifted(now, -30.0 * 86400.0).iso();
        let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
        for row in ledger.query("SELECT tier FROM runs WHERE started >= ?1", &[&since]) {
            let tier = match &row[0] {
                rusqlite::types::Value::Text(tier) => tier.as_str(),
                _ => "",
            };
            let owner = if tier.starts_with("gemini") { "gemini" } else if tier.starts_with("gpt-") { "codex" } else { "claude" };
            *counts.entry(owner).or_default() += 1;
        }
        let runs = counts.get(name.as_str()).copied().unwrap_or(0);
        let total: i64 = counts.values().sum();
        let run_share = if total > 0 { 100.0 * runs as f64 / total as f64 } else { 0.0 };
        let spend: f64 = full.values().sum();
        let cost_share = 100.0 * full.get(name.as_str()).copied().unwrap_or(0.0) / if spend == 0.0 { 1.0 } else { spend };
        let verdict = if run_share >= cost_share { "earned its full price" } else { "did not earn its full price" };
        let confirmed = if account.get("promo_end_confirmed").is_some_and(crate::router::ctx::truthy) { "confirmed" } else { "unconfirmed" };
        lines.push(format!(
            "Blocked: decision for Tom: {name} promotion (USD {}/month) ends {end} ({left} days, {confirmed}); at USD {}/month it is {cost_share:.1}% of plan spend and ran {run_share:.1}% of runs in 30 days ({runs} of {total}): {verdict}; keep it at full price?",
            field(account, "promo_price"),
            field(account, "monthly_price"),
        ));
    }
    lines
}

fn show(reference: &str, tier: &Tier, entry: &Value, measured: &Measured) {
    let empty = Value::Object(Default::default());
    let value = entry.get("value").unwrap_or(&empty);
    println!("  {reference}: {} {}", tier.model, tier.effort);
    if let Some(restriction) = entry.get("restriction").filter(|value| crate::router::ctx::truthy(value)) {
        println!("    restriction: {}", text(restriction));
    }
    let strengths: Vec<String> = entry["strengths"].as_array().into_iter().flatten().map(text).collect();
    println!("    strengths: {}", strengths.join("; "));
    let currency = field(value, "currency");
    let options: Vec<String> = value["monthly_price_options"].as_array().into_iter().flatten().map(text).collect();
    let price = field(value, "monthly_price");
    let monthly = if price == "unknown" { "monthly price unknown".to_owned() } else { format!("{currency} {price}/month") };
    let public = if options.is_empty() { String::new() } else { format!("; public options {} {currency}/month", options.join("/")) };
    println!(
        "    value: {}; {monthly} ({}{public}); {}, checked {}",
        field(value, "plan"), field(value, "monthly_price_note"), field(value, "source"), field(value, "checked")
    );
    if let Some(api) = value.get("api").filter(|value| crate::router::ctx::truthy(value)) {
        println!(
            "    API list comparison: {} {} input / {} output per million tokens ({}); {} input / {} output ({}); {}, checked {}",
            field(api, "currency"), field(api, "input_per_million"), field(api, "output_per_million"), field(api, "context"),
            field(api, "long_input_per_million"), field(api, "long_output_per_million"), field(api, "long_context"),
            field(api, "source"), field(api, "checked")
        );
    }
    println!(
        "    discounts: {}; expiry {}; {}, checked {}",
        field(value, "discounts"), field(value, "discount_expiry"), field(value, "discount_source"), field(value, "discount_checked")
    );
    match entry["benchmarks"].as_array().and_then(|list| list.first()) {
        Some(best) => {
            let result: Vec<String> = ["rank", "score"].iter().filter_map(|name| best.get(*name).map(|found| format!("{name} {}", text(found)))).collect();
            println!("    best benchmark: {}, {}; {}, checked {}", field(best, "name"), result.join(", "), field(best, "source"), field(best, "checked"));
        }
        None => println!("    best benchmark: none verified for this model and effort"),
    }
    match measured {
        Err(_) => println!("    measured: unavailable"),
        Ok(counts) => {
            let (runs, done) = counts.get(&run_tier(tier)).copied().unwrap_or((0, 0));
            let share = if runs > 0 { format!("{:.1}%", 100.0 * done as f64 / runs as f64) } else { "n/a (zero samples)".to_owned() };
            println!("    measured: {runs} runs; {done} done; done share {share}");
        }
    }
}

pub(super) fn catalog(document: &DocumentMut, tiers: &[Tier], usable: &[String], providers: &[String], measured: &Measured) -> Result<(), String> {
    let models = document.get("models").and_then(|models| models.as_table_like()).ok_or("catalog has no models")?;
    println!("catalog (hand-edited; API prices are comparison only, separate from plan usage):");
    for tier in tiers.iter().filter(|tier| usable.iter().any(|name| name == provider(&tier.key))) {
        let entry = models.get(&tier.key).map(json_value).ok_or_else(|| format!("catalog has no model {}", tier.key))?;
        show(&tier.key, tier, &entry, measured);
    }
    let mut others: Vec<(String, Vec<Value>)> = Vec::new();
    for (reference, item) in models.iter() {
        let owner = provider(reference);
        if providers.iter().any(|name| name == owner) {
            continue;
        }
        match others.iter_mut().find(|(known, _)| known == owner) {
            Some((_, entries)) => entries.push(json_value(item)),
            None => others.push((owner.to_owned(), vec![json_value(item)])),
        }
    }
    for (owner, entries) in others {
        let first = &entries[0];
        if !first.get("model").is_some_and(crate::router::ctx::truthy) {
            let note = first.get("note").map_or("no model yet".to_owned(), text);
            println!("{owner} (no staffing band): not set up ({note})");
            continue;
        }
        let value = &first["value"];
        println!(
            "{owner} (no staffing band): {}, {} {}/month; {}, checked {}",
            field(value, "plan"), field(value, "currency"), field(value, "monthly_price"), field(value, "source"), field(value, "checked")
        );
        if let Some(restriction) = first.get("restriction").filter(|value| crate::router::ctx::truthy(value)) {
            println!("  restriction: {}", text(restriction));
        }
        for entry in &entries {
            let key = format!("{} {}", field(entry, "model"), field(entry, "effort"));
            let (runs, done) = measured.as_ref().ok().and_then(|counts| counts.get(&key).copied()).unwrap_or((0, 0));
            let bench = match entry["benchmarks"].as_array().and_then(|list| list.first()) {
                Some(best) => format!("{} rank {}, checked {}", field(best, "name").split(" (").next().unwrap_or_default(), field(best, "rank"), field(best, "checked")),
                None => "no benchmark".to_owned(),
            };
            println!("  {key}: {bench}; measured {runs} runs, {done} done");
        }
    }
    Ok(())
}
