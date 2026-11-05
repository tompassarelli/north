//! The fleet's ports onto the ledger host: each asks the ledger host over the threads client and
//! falls back to the fleet store on this machine when this machine is the ledger host.
use crate::error::{NorthError, NorthResult};
use crate::fleet::jobs::JobLeases;
use crate::fleet::model::Machine;
use crate::fleet::store::{Claim, Exchange, Exchanged, LedgerHost, Registered, Registrar, Registration, SessionRow, Store, WaitKey};
use crate::threads::client;

const EXCHANGED: &[&str] = &["north", "fleet", "row", "exchange"];
const SETTLED: &[&str] = &["north", "fleet", "row", "settle"];
const REGISTER: &[&str] = &["north", "fleet", "row", "register"];
const LEASED: &[&str] = &["north", "fleet", "row", "leased"];

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

pub(super) struct LedgerRegistrar {
    far_machine: fn(&str) -> bool,
}

impl LedgerRegistrar {
    pub(super) fn new(far_machine: fn(&str) -> bool) -> Self {
        Self { far_machine }
    }
}

impl Registrar for LedgerRegistrar {
    fn register(&self, row: &SessionRow, claim: Claim) -> NorthResult<()> {
        let registration = serde_json::to_vec(&Registration { row: row.clone(), claim })?;
        let answer = client::ask_ledger_host(REGISTER, &registration)
            .map_err(|error| NorthError::Refused(format!("a launch registers {} in the fleet store on the ledger host first, and it did not answer: {}", row.name, error.user_message())))?;
        match answer {
            Some(answer) => serde_json::from_slice::<Registered>(&answer)?.refused.map_or(Ok(()), |why| Err(NorthError::Refused(why))),
            None if (self.far_machine)(&row.host) => Err(NorthError::Refused(format!(
                "{} runs only the CLIs other hosts place on it and keeps no fleet registry of its own, so it cannot register {}; launch from the host that places work here",
                row.host, row.name
            ))),
            None => Store::open(&client::ledger_path()?)?.register(row, claim),
        }
    }
}

pub(super) struct LedgerLeases;

impl JobLeases for LedgerLeases {
    fn leased(&self, session: &str, _machine: Option<&Machine>) -> NorthResult<bool> {
        let words: Vec<&str> = LEASED.iter().copied().chain([session]).collect();
        leased_by(client::ask_ledger_host(&words, &[]), || Store::open(&client::ledger_path()?)?.job_leased(session))
    }
}

fn leased_by(answer: NorthResult<Option<Vec<u8>>>, local: impl FnOnce() -> NorthResult<bool>) -> NorthResult<bool> {
    match answer? {
        Some(answer) => match String::from_utf8_lossy(&answer).trim() {
            "leased" => Ok(true),
            "free" => Ok(false),
            other => Err(NorthError::Refused(format!("the ledger host answered {other:?} to a job lease read, not leased or free"))),
        },
        None => local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answered(text: &str) -> NorthResult<Option<Vec<u8>>> {
        Ok(Some(text.as_bytes().to_vec()))
    }

    #[test]
    fn the_ledger_hosts_answer_is_read_as_leased_or_free() {
        assert!(leased_by(answered("leased\n"), || panic!("the ledger host answered")).unwrap());
        assert!(!leased_by(answered("free\n"), || panic!("the ledger host answered")).unwrap());
    }

    #[test]
    fn an_unreadable_answer_is_an_error_and_never_free() {
        assert!(leased_by(answered(""), || Ok(false)).is_err());
        assert!(leased_by(answered("Permission denied"), || Ok(false)).is_err());
        assert!(leased_by(Err(NorthError::Refused("ledger-host answered exit 255".into())), || Ok(false)).is_err());
        assert!(leased_by(Err(NorthError::Refused("ledger-host timed out; remote outcome unknown".into())), || Ok(false)).is_err());
    }

    #[test]
    fn this_machine_being_the_ledger_host_reads_its_own_store_and_a_store_that_does_not_open_is_an_error() {
        assert!(leased_by(Ok(None), || Ok(true)).unwrap());
        assert!(!leased_by(Ok(None), || Ok(false)).unwrap());
        assert!(leased_by(Ok(None), || Err(NorthError::Refused("the ledger store did not open".into()))).is_err());
    }
}
