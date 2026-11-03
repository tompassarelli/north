use std::io::Read;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::store::{Exchange, FleetRow, LedgerHost, Store, WaitKey, WaitRequest};
use crate::threads::client;

use super::spool;

const USAGE: &str = "Usage: north fleet row put < ROW_JSON_OR_ARRAY | north fleet row exchange < EXCHANGE_JSON | north fleet row register < REGISTRATION_JSON | north fleet row wait < WAIT_JSON | north fleet row settle < WAIT_KEYS_JSON | north fleet row leased SESSION";
const FORWARDED_PUT: &[&str] = &["north", "fleet", "row", "put"];
const FORWARDED_WAIT: &[&str] = &["north", "fleet", "row", "wait"];

pub(super) fn run(fleet: &Fleet, verb: &str, words: &[&str]) -> NorthResult<()> {
    match (verb, words) {
        ("row", ["put"]) => put(),
        ("row", ["exchange"]) => serve_exchange(),
        ("row", ["register"]) => serve_registration(),
        ("row", ["wait"]) => wait(),
        ("row", ["settle"]) => settle(),
        ("row", ["leased", session]) => leased(session),
        ("spool", _) => spool::run(fleet, words),
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
    match client::forward_to_ledger_host(FORWARDED_PUT, &input)? {
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

fn settle() -> NorthResult<()> {
    let waits: Vec<WaitKey> = serde_json::from_slice(&stdin()?)?;
    Store::open(&client::ledger_path()?)?.settle_all(&waits)
}

fn leased(session: &str) -> NorthResult<()> {
    println!("{}", if Store::open(&client::ledger_path()?)?.job_leased(session)? { "leased" } else { "free" });
    Ok(())
}

fn serve_registration() -> NorthResult<()> {
    let registered = Store::open(&client::ledger_path()?)?.serve_registration(&stdin()?)?;
    println!("{}", serde_json::to_string(&registered)?);
    Ok(())
}

fn wait() -> NorthResult<()> {
    let input = stdin()?;
    record(&WaitRequest::parse(&input)?, &input)
}

pub(super) fn record_wait(request: &WaitRequest) -> NorthResult<()> {
    record(request, &serde_json::to_vec(request)?)
}

fn record(request: &WaitRequest, input: &[u8]) -> NorthResult<()> {
    match client::forward_to_ledger_host(FORWARDED_WAIT, input)? {
        Some(0) => Ok(()),
        Some(code) => Err(NorthError::Refused(format!("the ledger host did not record the wait (exit {code}); retry"))),
        None => Store::open(&client::ledger_path()?)?.wait(request),
    }
}
