use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::{Exchange, Exchanged, LedgerHost, OverSsh, Store, WaitKey};
use crate::fleet::tick::{FarHost, Pulls, tick};
use crate::ops::pacing::placement::{config, gate};
use crate::threads::client;

use super::fleet_waits::Probes;

const USAGE: &str = "Usage: north fleet tick";
const BUDGET: Duration = Duration::from_secs(15);
const EXCHANGED: &[&str] = &["north", "fleet", "row", "exchange"];
const SETTLED: &[&str] = &["north", "fleet", "row", "settle"];

pub(super) struct Ledger;

impl LedgerHost for Ledger {
    fn exchange(&mut self, exchange: &Exchange) -> NorthResult<Exchanged> {
        match client::ask_ledger_host(EXCHANGED, &serde_json::to_vec(exchange)?)? {
            Some(answer) => Ok(serde_json::from_slice(&answer)?),
            None => Store::open(&client::ledger_path()?)?.exchange(exchange),
        }
    }

    fn settle(&mut self, waits: &[WaitKey]) -> NorthResult<()> {
        match client::forward_to_ledger_host(SETTLED, &serde_json::to_vec(waits)?)? {
            Some(0) => Ok(()),
            Some(code) => Err(NorthError::Refused(format!("the ledger host did not settle the waits (exit {code}); the next tick retries"))),
            None => Store::open(&client::ledger_path()?)?.settle_all(waits),
        }
    }
}

fn always_pulled(fleet: &Fleet) -> Vec<FarHost> {
    config().map(|cfg| gate::spool_pulls(&cfg, fleet.host())).unwrap_or_default().into_iter().map(|(name, ssh)| FarHost { name, ssh }).collect()
}

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    if !words.is_empty() {
        return Err(NorthError::Usage(USAGE.into()));
    }
    let pulls = Pulls { spools: &OverSsh::default(), always: always_pulled(fleet) };
    let ticked = tick(fleet, &mut Ledger, &pulls, &Probes, now_ms(), BUDGET)?;
    ticked.lines().iter().for_each(|line| println!("{line}"));
    Ok(())
}
