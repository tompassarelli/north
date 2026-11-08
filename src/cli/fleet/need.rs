//! `north fleet need HOST REASON [--item REPO#N]`: a worker that cannot finish its box here records the host or local need that
//! can, and dispatch places the next try there.
use crate::dispatch::{is_need, register_need};
use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::SESSION_ENV;
use crate::platform::attribution::{self, Cause, Harness};
use crate::threads::client;

const USAGE: &str = "Usage: north fleet need HOST REASON [--item REPO#N] (HOST is a host or local need such as laptop or native; the session is $NORTH_FLEET_SESSION, its issue the one its launch recorded)";

#[derive(Debug, PartialEq)]
struct Need<'a> {
    need: &'a str,
    item: Option<&'a str>,
    reason: String,
}

impl<'a> Need<'a> {
    fn parse(words: &[&'a str]) -> NorthResult<Self> {
        let (need, rest) = words.split_first().ok_or_else(|| NorthError::Usage(USAGE.into()))?;
        let mut item = None;
        let mut reason = Vec::new();
        let mut rest = rest.iter();
        while let Some(word) = rest.next() {
            match *word {
                "--item" if item.is_none() => item = Some(*rest.next().ok_or_else(|| NorthError::Usage(USAGE.into()))?),
                word => reason.push(word),
            }
        }
        let reason = reason.join(" ");
        if !is_need(need) || reason.trim().is_empty() {
            return Err(NorthError::Usage(USAGE.into()));
        }
        Ok(Self { need, item, reason })
    }

    fn forwarded<'b>(&'b self, session: &'b str) -> Vec<&'b str> {
        ["north", "fleet", "dispatch", "need", session, self.need].into_iter().chain(self.item).collect()
    }
}

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let session = std::env::var(SESSION_ENV).ok().filter(|name| !name.is_empty()).ok_or_else(|| NorthError::Usage(format!("{USAGE}; ${SESSION_ENV} is unset")))?;
    let asked = Need::parse(words)?;
    let answer = match client::ask_ledger_host(&asked.forwarded(&session), asked.reason.as_bytes())? {
        Some(answer) => String::from_utf8_lossy(&answer).trim().to_owned(),
        None => register_need(&session, asked.need, &asked.reason, asked.item).map_err(NorthError::Refused)?,
    };
    attribution::record(&session, "host-need", Cause::Harness(Harness::Placement), &format!("{}: {}", asked.need, asked.reason.trim()));
    println!("{answer}");
    Ok(())
}

#[cfg(test)]
#[path = "need_tests.rs"]
mod tests;
