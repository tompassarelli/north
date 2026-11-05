//! `north fleet spend [NAME | --self]`: the API-equivalent spend a session has run up so far,
//! the ledger's finished turns plus the turn still open. A session another host holds answers by
//! message to the asking session.
use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::{Fleet, Spent};
use crate::fleet::runtime::SESSION_ENV;

const USAGE: &str = "Usage: north fleet spend [NAME | --self] (--self and no NAME read the session name from $NORTH_FLEET_SESSION)";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let own = std::env::var(SESSION_ENV).ok().filter(|name| !name.is_empty());
    let name = match (words, own.as_deref()) {
        ([] | ["--self"], Some(own)) => own.to_owned(),
        ([] | ["--self"], None) => return Err(NorthError::Usage(format!("{USAGE}; ${SESSION_ENV} is not set"))),
        ([name], _) if !name.starts_with("--") => (*name).to_owned(),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    match fleet.spend_or_ask(&name, own.as_deref())? {
        Spent::Here(line) => println!("{line}"),
        Spent::Asked { id, host } => println!("spooled SPEND row {id}: {host} holds {name} and answers with a message to {} at its next tick", own.as_deref().unwrap_or_default()),
    }
    Ok(())
}
