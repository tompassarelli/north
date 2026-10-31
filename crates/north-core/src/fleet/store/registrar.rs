use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::threads::client;

use super::{Claim, SessionRow, Store};

const REGISTER: &[&str] = &["north", "fleet", "row", "register"];

pub trait Registrar: Send + Sync {
    fn register(&self, row: &SessionRow, claim: Claim) -> NorthResult<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registration {
    pub row: SessionRow,
    pub claim: Claim,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Registered {
    pub refused: Option<String>,
}

pub struct LedgerRegistrar {
    far_machine: fn(&str) -> bool,
}

impl LedgerRegistrar {
    pub fn new(far_machine: fn(&str) -> bool) -> Self {
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

impl Store {
    pub fn serve_registration(&mut self, input: &[u8]) -> NorthResult<Registered> {
        let registration: Registration = serde_json::from_slice(input)?;
        match self.register(&registration.row, registration.claim) {
            Ok(()) => Ok(Registered::default()),
            Err(NorthError::Refused(why)) => Ok(Registered { refused: Some(why) }),
            Err(error) => Err(error),
        }
    }
}
