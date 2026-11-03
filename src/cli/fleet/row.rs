use std::io::Read;

use crate::error::{NorthError, NorthResult};
use crate::fleet::peer_row;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::{Exchange, Exchanged, FleetRow, LedgerHost, Registered, Registration, SessionRow, Store, WaitKey, WaitRequest};
use crate::threads::client;

use super::spool;

const USAGE: &str = "Usage: north fleet row put [--peer HOST] < ROW_JSON_OR_ARRAY | north fleet row exchange [--peer HOST] < EXCHANGE_JSON | north fleet row register [--peer HOST] < REGISTRATION_JSON | north fleet row wait [--peer HOST] < WAIT_JSON | north fleet row settle [--peer HOST] < WAIT_KEYS_JSON | north fleet row leased SESSION";
const FORWARDED_PUT: &[&str] = &["north", "fleet", "row", "put"];
const FORWARDED_WAIT: &[&str] = &["north", "fleet", "row", "wait"];
const REMOTE_PEER: &str = "NORTH_REMOTE_PEER";

pub(super) fn run(fleet: &Fleet, verb: &str, words: &[&str]) -> NorthResult<()> {
    let (served, peer) = match (verb, words) {
        ("row", ["leased", session]) => return leased(session),
        ("spool", _) => return spool::run(fleet, words),
        ("row", [served]) => (*served, None),
        ("row", [served, "--peer", peer]) => (*served, Some(*peer)),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    let peer = entered(peer, |name| std::env::var(name).ok())?;
    match served {
        "put" => put(peer),
        "exchange" => serve_exchange(peer),
        "register" => serve_registration(peer),
        "wait" => wait(peer),
        "settle" => settle(peer),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

pub(in crate::cli) fn entered(peer: Option<&str>, env: impl Fn(&str) -> Option<String>) -> NorthResult<Option<&str>> {
    let set = |name: &str| env(name).filter(|value| !value.is_empty());
    if let Some(forced) = set(REMOTE_PEER)
        && peer != Some(forced.as_str())
    {
        return Err(NorthError::Refused(format!("a fleet row verb entered for peer {forced} must name --peer {forced}")));
    }
    if peer.is_none() && set("SSH_ORIGINAL_COMMAND").is_some() {
        return Err(NorthError::Refused("a fleet row verb entered through a forced ssh command must name --peer; only this host's own calls run unchecked".into()));
    }
    Ok(peer)
}

fn stdin() -> NorthResult<Vec<u8>> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    Ok(input)
}

fn ledger_sessions() -> NorthResult<Vec<SessionRow>> {
    Store::open(&client::ledger_path()?)?.sessions()
}

pub(in crate::cli) fn forwarded<'a>(words: &[&'a str], peer: Option<&'a str>) -> Vec<&'a str> {
    words.iter().copied().chain(peer.into_iter().flat_map(|peer| ["--peer", peer])).collect()
}

fn put(peer: Option<&str>) -> NorthResult<()> {
    let (rows, input) = admitted_put(&stdin()?, peer, ledger_sessions)?;
    match client::forward_to_ledger_host(&forwarded(FORWARDED_PUT, peer), &input)? {
        Some(0) => Ok(()),
        Some(code) => Err(NorthError::Refused(format!("the ledger host did not confirm the commit (exit {code}); retry, a row id commits once"))),
        None => {
            Store::open(&client::ledger_path()?)?.put(&rows)?;
            rows.iter().for_each(|row| println!("committed {}", row.id));
            Ok(())
        }
    }
}

fn serve_exchange(peer: Option<&str>) -> NorthResult<()> {
    let mut store = Store::open(&client::ledger_path()?)?;
    let exchanged = served_exchange(&mut store, &stdin()?, peer)?;
    println!("{}", serde_json::to_string(&exchanged)?);
    Ok(())
}

pub(in crate::cli) fn admitted_put(input: &[u8], peer: Option<&str>, sessions: impl FnOnce() -> NorthResult<Vec<SessionRow>>) -> NorthResult<(Vec<FleetRow>, Vec<u8>)> {
    let rows = FleetRow::parse(input)?;
    let Some(peer) = peer else { return Ok((rows, input.to_vec())) };
    let rows = peer_row::admit_all(&rows, peer, &sessions()?)?;
    let input = serde_json::to_vec(&rows)?;
    Ok((rows, input))
}

pub(in crate::cli) fn admitted_exchange(input: &[u8], peer: Option<&str>, store: &Store) -> NorthResult<Exchange> {
    let exchange: Exchange = serde_json::from_slice(input)?;
    match peer {
        Some(peer) => peer_row::admit_exchange(exchange, peer, &store.sessions()?, now_ms()),
        None => Ok(exchange),
    }
}

pub(in crate::cli) fn served_exchange(store: &mut Store, input: &[u8], peer: Option<&str>) -> NorthResult<Exchanged> {
    let exchange = admitted_exchange(input, peer, store)?;
    let exchanged = store.exchange(&exchange)?;
    Ok(match peer {
        Some(peer) => peer_row::seen_by(exchanged, peer),
        None => exchanged,
    })
}

pub(in crate::cli) fn admitted_settle(input: &[u8], peer: Option<&str>, sessions: impl FnOnce() -> NorthResult<Vec<SessionRow>>) -> NorthResult<Vec<WaitKey>> {
    let waits: Vec<WaitKey> = serde_json::from_slice(input)?;
    if let Some(peer) = peer {
        peer_row::admit_settle(&waits, peer, &sessions()?)?;
    }
    Ok(waits)
}

fn settle(peer: Option<&str>) -> NorthResult<()> {
    let mut store = Store::open(&client::ledger_path()?)?;
    let waits = admitted_settle(&stdin()?, peer, || store.sessions())?;
    store.settle_all(&waits)
}

fn leased(session: &str) -> NorthResult<()> {
    println!("{}", if Store::open(&client::ledger_path()?)?.job_leased(session)? { "leased" } else { "free" });
    Ok(())
}

pub(in crate::cli) fn served_registration(store: &mut Store, input: &[u8], peer: Option<&str>) -> NorthResult<Registered> {
    if let Some(peer) = peer {
        let registration: Registration = serde_json::from_slice(input)?;
        return match peer_row::admit_registration(&registration, peer, &store.sessions()?, now_ms()) {
            Ok(admitted) => store.serve_registration(&serde_json::to_vec(&admitted)?),
            Err(NorthError::Refused(why)) => Ok(Registered { refused: Some(why) }),
            Err(error) => Err(error),
        };
    }
    store.serve_registration(input)
}

fn serve_registration(peer: Option<&str>) -> NorthResult<()> {
    let registered = served_registration(&mut Store::open(&client::ledger_path()?)?, &stdin()?, peer)?;
    println!("{}", serde_json::to_string(&registered)?);
    Ok(())
}

pub(in crate::cli) fn admitted_wait(input: &[u8], peer: Option<&str>, sessions: impl FnOnce() -> NorthResult<Vec<SessionRow>>) -> NorthResult<WaitRequest> {
    let request = WaitRequest::parse(input)?;
    if let Some(peer) = peer {
        peer_row::admit_wait_sessions([request.session.as_str()], peer, &sessions()?)?;
    }
    Ok(request)
}

fn wait(peer: Option<&str>) -> NorthResult<()> {
    let input = stdin()?;
    record(&admitted_wait(&input, peer, ledger_sessions)?, &input, peer)
}

pub(super) fn record_wait(request: &WaitRequest) -> NorthResult<()> {
    record(request, &serde_json::to_vec(request)?, None)
}

fn record(request: &WaitRequest, input: &[u8], peer: Option<&str>) -> NorthResult<()> {
    match client::forward_to_ledger_host(&forwarded(FORWARDED_WAIT, peer), input)? {
        Some(0) => Ok(()),
        Some(code) => Err(NorthError::Refused(format!("the ledger host did not record the wait (exit {code}); retry"))),
        None => Store::open(&client::ledger_path()?)?.wait(request),
    }
}
