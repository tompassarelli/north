use std::io::{Read, Write};

use serde_json::json;

mod admission;
pub(crate) mod agent_load;
pub(crate) mod laptop_auto;
mod deadline;
mod lease_deadline;
#[cfg(test)]
mod agent_load_tests;
#[cfg(test)]
mod tests;

use admission::Facts;

pub(crate) fn run(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("admission") if arguments.len() == 1 => {}
        Some("laptop-auto") => return laptop_auto::run(&arguments[1..]),
        Some("deadline") => return deadline::run(&arguments[1..]),
        _ => {
            eprintln!("usage: north capacity admission < FACTS.json | laptop-auto [--dry-run] | deadline LEASE_FILE SECONDS -- COMMAND...");
            return 2;
        }
    }
    let mut input = String::new();
    let outcome = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Facts>(&input).map_err(|error| error.to_string()));
    match outcome {
        Ok(mut facts) => {
            facts.gaming |= agent_load::gaming_marked(&agent_load::capacity_root());
            let _ = writeln!(std::io::stdout(), "{}", json!({ "reason": admission::decide(&facts) }));
            0
        }
        Err(message) => {
            eprintln!("north capacity: {message}");
            2
        }
    }
}
