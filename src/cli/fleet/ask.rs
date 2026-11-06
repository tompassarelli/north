use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::cli::send;
use super::usage::usage;

const DEFAULT_ASK_SECS: u64 = 120;

pub(super) fn run(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let (question, timeout) = match words {
        [question @ .., "--timeout", secs] => (question, secs.parse().map_err(|_| usage())?),
        question => (question, DEFAULT_ASK_SECS),
    };
    if question.is_empty() {
        return Err(usage());
    }
    let (flags, question) = send::leading_flags(question)?;
    if question.is_empty() {
        return Err(usage());
    }
    let from = send::signer(fleet, flags.from, name)?;
    match fleet.ask(&from, name, &question.join(" "), Duration::from_secs(timeout))? {
        Some(reply) => {
            println!("{reply}");
            Ok(())
        }
        None => Err(NorthError::Rejected(format!("{name} did not answer within {timeout} s"))),
    }
}

