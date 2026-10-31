use std::io::Read;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::store::{Exchange, FleetRow, LedgerHost, Store};
use crate::threads::client;

use super::fleet_spool;
use super::fleet_tick;

const USAGE: &str = "Usage: north fleet row put < ROW_JSON_OR_ARRAY | north fleet row exchange < EXCHANGE_JSON";
const FORWARDED: &[&str] = &["north", "fleet", "row", "put"];

pub(super) fn run(fleet: &Fleet, verb: &str, words: &[&str]) -> NorthResult<()> {
    match (verb, words) {
        ("row", ["put"]) => put(),
        ("row", ["exchange"]) => serve_exchange(),
        ("spool", _) => fleet_spool::run(fleet, words),
        ("tick", _) => fleet_tick::run(fleet, words),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

fn stdin() -> NorthResult<Vec<u8>> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    Ok(input)
}

fn put() -> NorthResult<()> {
    let input = stdin()?;
    let rows = FleetRow::parse(&input)?;
    match client::forward_to_ledger_host(FORWARDED, &input)? {
        Some(0) => Ok(()),
        Some(code) => Err(NorthError::Refused(format!("the ledger host did not confirm the commit (exit {code}); retry, a row id commits once"))),
        None => {
            Store::open(&client::ledger_path()?)?.put(&rows)?;
            rows.iter().for_each(|row| println!("committed {}", row.id));
            Ok(())
        }
    }
}

fn serve_exchange() -> NorthResult<()> {
    let exchange: Exchange = serde_json::from_slice(&stdin()?)?;
    let exchanged = Store::open(&client::ledger_path()?)?.exchange(&exchange)?;
    println!("{}", serde_json::to_string(&exchanged)?);
    Ok(())
}
