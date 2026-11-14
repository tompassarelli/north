//! The client side of `north runtime host-op` (north#1485, host_op_request.rs): the words the caller typed go to the outside broker as they are, once they read as one of its three requests, and the broker tells the caller what was done.

use super::broker;
use super::host_op_request::{Ask, VERB};

/// `north runtime host-op state HOST UNIT | restart HOST UNIT | sync COPY`; its exit code: the broker's (0 done, 126 refused), 2 for words that are no such request, 1 when the broker cannot be asked.
pub fn command(arguments: &[String]) -> u8 {
    let words: Vec<String> = std::iter::once(VERB.to_owned()).chain(arguments.iter().cloned()).collect();
    if let Some(Err(why)) = Ask::asked(&words) {
        eprintln!("north runtime host-op: {why}");
        return 2;
    }
    broker::request_check(&words).unwrap_or_else(|why| {
        eprintln!("{why}");
        1
    })
}
