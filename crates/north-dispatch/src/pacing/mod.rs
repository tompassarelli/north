use crate::pace::target;
use crate::error::NorthResult;
use crate::fleet::roster::Roster;
use std::collections::BTreeMap;
use std::io::Write;

mod command;
pub mod placement;
#[cfg(test)]
mod placement_gate_tests;
mod process;
mod readout;
mod sampling;
#[cfg(test)]
mod sampling_tests;
pub mod spawn_gate;
mod storage;

pub use sampling::TokenUsage;

#[derive(Clone, Copy)]
pub struct FleetFacts {
    pub roster: fn() -> NorthResult<Roster>,
    pub machine_load: fn() -> NorthResult<BTreeMap<String, i64>>,
}

pub fn run(arguments: &[String], fleet: FleetFacts, token_usage: TokenUsage) -> u8 {
    sampling::install_token_usage(token_usage);
    if let [command, provider] = arguments
        && command == "gate" {
            return gate(provider);
        }
    match arguments.first().map(String::as_str) {
        Some("place") => finish(placement::cli(&arguments[1..], fleet), "north place"),
        Some("sample") => finish(command::cli(&arguments[1..]), "north pace"),
        _ => request(),
    }
}
fn finish(outcome: Result<(), String>, label: &str) -> u8 {
    match outcome {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{label}: {error}");
            2
        }
    }
}
fn gate(provider: &str) -> u8 {
    match sampling::sample(Some(provider)) {
        Ok(gate) => {
            if let Some(reason) = gate[provider]["refusal"].as_str() {
                println!("{reason}");
                3
            } else {
                0
            }
        }
        Err(error) => {
            eprintln!("north pace: {error}");
            2
        }
    }
}
pub fn answer(value: &serde_json::Value) -> Result<serde_json::Value, String> {
    if let Some(answer) = command::operation(value)? {
        return Ok(answer);
    }
    if let Some(answer) = crate::router::ops::answer(value) {
        return answer;
    }
    crate::pace::answer_value(value.clone())
}
fn request() -> u8 {
    let outcome = command::input().and_then(|value| answer(&value));
    match outcome {
        Ok(value) => {
            let _ = writeln!(std::io::stdout(), "{value}");
            0
        }
        Err(message) => {
            eprintln!("north pace: {message}");
            2
        }
    }
}
