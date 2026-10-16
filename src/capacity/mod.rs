use std::io::{Read, Write};

use serde_json::json;

mod admission;
#[cfg(test)]
mod tests;

use admission::Facts;

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments != ["admission"] {
        eprintln!("usage: north capacity admission < FACTS.json");
        return 2;
    }
    let mut input = String::new();
    let outcome = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Facts>(&input).map_err(|error| error.to_string()));
    match outcome {
        Ok(facts) => {
            let _ = writeln!(std::io::stdout(), "{}", json!({ "reason": admission::decide(&facts) }));
            0
        }
        Err(message) => {
            eprintln!("north capacity: {message}");
            2
        }
    }
}
