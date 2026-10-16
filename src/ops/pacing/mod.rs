use crate::pace::{admission, control, target, window};
use std::io::Write;

mod command;
#[cfg(test)]
mod grant_tests;
mod grants;
mod gate_order;
mod placement;
#[cfg(test)]
mod placement_tests;
mod process;
mod readout;
mod reservation;
mod sampling;
#[cfg(test)]
mod sampling_tests;
mod storage;

pub(crate) fn run(_arguments: &[String]) -> u8 {
    if let [command, provider] = _arguments {
        if command == "gate" {
            return match sampling::sample(Some(provider), 2, true) {
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
            };
        }
    }
    if _arguments.first().is_some_and(|s| s == "place") {
        return match placement::cli(&_arguments[1..]) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("north place: {error}");
                2
            }
        };
    }
    if _arguments
        .first()
        .is_some_and(|s| s == "sample" || s == "grant")
    {
        let args = if _arguments[0] == "sample" {
            &_arguments[1..]
        } else {
            _arguments
        };
        return match command::cli(args) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("north pace: {error}");
                2
            }
        };
    }
    let outcome = command::input().and_then(|value| {
        if let Some(answer) = command::operation(&value)? {
            return Ok(answer);
        }
        if let Some(answer) = placement::operation(&value)? {
            return Ok(answer);
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
