use std::io::Read;

use crate::error::{NorthError, NorthResult};
use crate::fleet::store::{FleetRow, Store};
use crate::threads::client;

const USAGE: &str = "Usage: north fleet row put < ROW_JSON_OR_ARRAY";
const FORWARDED: &[&str] = &["north", "fleet", "row", "put"];

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    match words {
        ["put"] => put(),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

fn put() -> NorthResult<()> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
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
