//! The coordinator's ordered chain of seats, read from harness/defaults.toml: each seat names the provider, account and model the coordinator moves to in turn. A seat on a provider marked public-only is refused, since the coordinator serves private repositories (north#1552).
use toml_edit::{DocumentMut, Value};

use crate::platform::harness;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub provider: String,
    pub account: String,
    pub model: String,
}

pub fn read() -> Result<Vec<Seat>, String> {
    let defaults: DocumentMut = harness::table(harness::DEFAULTS).parse().map_err(|error| format!("harness/defaults.toml: {error}"))?;
    parse(&defaults)
}

/// The config check a change to harness/defaults.toml meets before it lands: the chain must read, and no seat may name a public-only provider.
pub fn validate(text: &str) -> Result<(), String> {
    let defaults: DocumentMut = text.parse().map_err(|error| format!("harness/defaults.toml: {error}"))?;
    parse(&defaults).map(drop)
}

pub fn parse(defaults: &DocumentMut) -> Result<Vec<Seat>, String> {
    let value = |name: &str| defaults.get(name).and_then(|entry| entry.get("value")).and_then(|entry| entry.as_value());
    let public_only: Vec<&str> = value("public_only_providers").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let seats = value("coordinator_chain").and_then(Value::as_array).ok_or("harness/defaults.toml has no coordinator_chain list")?;
    seats.iter().enumerate().map(|(index, seat)| seat_at(index + 1, seat, &public_only)).collect()
}

fn seat_at(number: usize, value: &Value, public_only: &[&str]) -> Result<Seat, String> {
    let field = |name: &str| {
        value
            .as_inline_table()
            .and_then(|table| table.get(name))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("coordinator_chain seat {number} has no {name}"))
    };
    let seat = Seat { provider: field("provider")?, account: field("account")?, model: field("model")? };
    if public_only.contains(&seat.provider.as_str()) {
        return Err(format!("coordinator_chain seat {number} names provider {}, which is public-only and may not hold the coordinator", seat.provider));
    }
    Ok(seat)
}

pub fn holds(chain: &[Seat], account: &str) -> bool {
    chain.iter().any(|seat| seat.account == account)
}

pub fn after<'a>(chain: &'a [Seat], account: &str) -> Option<&'a Seat> {
    let at = chain.iter().position(|seat| seat.account == account)?;
    chain.get(at + 1)
}
