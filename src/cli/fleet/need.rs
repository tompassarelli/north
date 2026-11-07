//! `north fleet need HOST REASON [--item REPO#N]`: a worker that cannot finish its box here records the host or local need that
//! can, and dispatch places the next try there.
use crate::dispatch::{is_need, register_need};
use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::SESSION_ENV;
use crate::threads::client;

const USAGE: &str = "Usage: north fleet need HOST REASON [--item REPO#N] (HOST is a host or local need such as laptop or native; the session is $NORTH_FLEET_SESSION, its issue the one its launch recorded)";

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let session = std::env::var(SESSION_ENV).ok().filter(|name| !name.is_empty()).ok_or_else(|| NorthError::Usage(format!("{USAGE}; ${SESSION_ENV} is unset")))?;
    let (need, rest) = words.split_first().ok_or_else(|| NorthError::Usage(USAGE.into()))?;
    let (item, reason) = match rest {
        ["--item", item, reason @ ..] => (Some(*item), reason),
        reason => (None, reason),
    };
    let reason = reason.join(" ");
    if !is_need(need) || reason.trim().is_empty() {
        return Err(NorthError::Usage(USAGE.into()));
    }
    let mut asked = vec!["north", "dispatch", "need", session.as_str(), need];
    asked.extend(item);
    let answer = match client::ask_ledger_host(&asked, reason.as_bytes())? {
        Some(answer) => String::from_utf8_lossy(&answer).trim().to_owned(),
        None => register_need(&session, need, &reason, item).map_err(NorthError::Refused)?,
    };
    println!("{answer}");
    Ok(())
}
