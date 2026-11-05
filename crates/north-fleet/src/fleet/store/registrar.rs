use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

use super::{Claim, SessionRow, Store};

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
