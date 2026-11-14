//! The client side of `north runtime eval-unit` (north#1414, eval_unit_request.rs): the words the caller typed go to the outside broker as they are, once they read as one of its three requests, and the broker tells the caller what was done.

use super::broker;
use super::eval_unit_request::{Ask, VERB};

/// `north runtime eval-unit start finish-proof LABEL LEVEL | state LABEL | stop LABEL`; its exit code: the broker's (0 done, 126 refused), 2 for words that are no such request, 1 when the broker cannot be asked.
pub fn command(arguments: &[String]) -> u8 {
    let words: Vec<String> = std::iter::once(VERB.to_owned()).chain(arguments.iter().cloned()).collect();
    if let Some(Err(why)) = Ask::asked(&words) {
        eprintln!("north runtime eval-unit: {why}");
        return 2;
    }
    broker::request_check(&words).unwrap_or_else(|why| {
        eprintln!("{why}");
        1
    })
}
