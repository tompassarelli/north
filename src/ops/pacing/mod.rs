use crate::error::NorthResult;
use crate::fleet::roster::Roster;
use crate::pace::{admission, control, target, window};
use std::io::Write;

mod command;
#[cfg(test)]
mod grant_tests;
mod grants;
mod gate_order;
pub(crate) mod placement;
#[cfg(test)]
mod placement_gate_tests;
#[cfg(test)]
mod placement_tests;
mod process;
mod readout;
#[cfg(test)]
mod readout_tests;
mod reservation;
mod sampling;
#[cfg(test)]
mod sampling_tests;
mod storage;

pub(crate) use sampling::TokenUsage;

pub(crate) fn run(arguments: &[String], roster: fn() -> NorthResult<Roster>, token_usage: TokenUsage) -> u8 {
    sampling::install_token_usage(token_usage);
    if let [command, provider] = arguments
        && command == "gate" {
            return gate(provider);
        }
    match arguments.first().map(String::as_str) {
        Some("place") => finish(placement::cli(&arguments[1..], roster), "north place"),
        Some("sample") => finish(command::cli(&arguments[1..]), "north pace"),
        Some("grant") => finish(command::cli(arguments), "north pace"),
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
    match sampling::sample(Some(provider), 2, true) {
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
fn request() -> u8 {
    let outcome = command::input().and_then(|value| {
        if let Some(answer) = command::operation(&value)? {
            return Ok(answer);
        }
        if let Some(answer) = placement::operation(&value)? {
            return Ok(answer);
        }
        if let Some(answer) = crate::router::ops::answer(&value) {
            return answer;
        }
        crate::pace::answer_value(value)
    });
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
