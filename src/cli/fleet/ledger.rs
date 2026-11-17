//! The fleet's ports onto the ledger host: each asks the ledger host over the threads client and
//! falls back to the fleet store on this machine when this machine is the ledger host.
use crate::error::{NorthError, NorthResult};
use crate::fleet::jobs::JobLeases;
use crate::fleet::model::Machine;
use crate::fleet::store::{Claim, Registered, Registrar, Registration, SessionRow, Store};
use crate::threads::client;
use crate::platform::orchestration;

const REGISTER: &[&str] = &["north", "fleet", "row", "register"];
const LEASED: &[&str] = &["north", "fleet", "row", "leased"];
const RETIRE: &[&str] = &["north", "fleet", "row", "retire"];

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
            .map_err(|error| NorthError::Refused(format!("a launch registers {} in the fleet store on the ledger host, and it did not answer: {}", row.name, error.user_message())))?;
        match answer {
            Some(answer) => serde_json::from_slice::<Registered>(&answer)?.refused.map_or(Ok(()), |why| Err(NorthError::Refused(why))),
            None if (self.far_machine)(&row.host) && !client::private_ledger() => Err(NorthError::Refused(format!(
                "{} runs only the CLIs other hosts place on it and keeps no fleet registry of its own, so it cannot register {}; launch from the host that places work here",
                row.host, row.name
            ))),
            None => Store::open(&client::ledger_path()?)?.register(row, claim),
        }
    }

    fn retire_unplaced(&self) -> NorthResult<Vec<String>> {
        match client::ask_ledger_host(RETIRE, &[])? {
            Some(answer) => Ok(serde_json::from_slice(&answer)?),
            None => placed_store()?.retire_unplaced(None),
        }
    }
}



pub(super) fn placed_store() -> NorthResult<Store> {
    Ok(Store::open(&client::ledger_path()?)?.placing(orchestration::placed_hosts(&orchestration::path())))
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
