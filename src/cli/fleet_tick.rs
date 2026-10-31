use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::{Exchange, Exchanged, LedgerHost, Store};
use crate::fleet::tick::tick;
use crate::threads::client;

const USAGE: &str = "Usage: north fleet tick";
const BUDGET: Duration = Duration::from_secs(15);
const EXCHANGED: &[&str] = &["north", "fleet", "row", "exchange"];

struct Ledger;

impl LedgerHost for Ledger {
    fn exchange(&mut self, exchange: &Exchange) -> NorthResult<Exchanged> {
        match client::ask_ledger_host(EXCHANGED, &serde_json::to_vec(exchange)?)? {
            Some(answer) => Ok(serde_json::from_slice(&answer)?),
            None => Store::open(&client::ledger_path()?)?.exchange(exchange),
        }
    }
}

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    if !words.is_empty() {
        return Err(NorthError::Usage(USAGE.into()));
    }
    let ticked = tick(fleet, &mut Ledger, now_ms(), BUDGET)?;
    ticked.lines().iter().for_each(|line| println!("{line}"));
    Ok(())
}
