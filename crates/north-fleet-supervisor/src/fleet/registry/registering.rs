use crate::error::NorthResult;
use crate::fleet::store::{Claim, SessionRow};

use super::Fleet;
use super::stopping::Halt;

impl Fleet {
    /// Registers `row` with the ledger: a take-over first, since it retires its predecessor, and a fresh launch beside its host start and first turn, answered when the returned wait is called.
    pub(super) fn register_beside<'scope, 'env>(&'env self, scope: &'scope std::thread::Scope<'scope, 'env>, row: &'env SessionRow, claim: Claim) -> NorthResult<impl FnOnce() -> NorthResult<()> + 'scope> {
        let registering = match claim {
            Claim::Fresh => Some(scope.spawn(move || self.registrar.register(row, claim))),
            Claim::TakeOver => {
                self.registrar.register(row, claim)?;
                None
            }
        };
        Ok(move || registering.map_or(Ok(()), |thread| thread.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))))
    }

    /// Stops a fresh session the ledger refused or never answered for: it registered beside its first turn, so its host is up and must end before its directory is discarded.
    pub(super) fn halt_unregistered(&self, name: &str) {
        if let Err(error) = self.halt(name, Halt::FailedStart) {
            eprintln!("north fleet: {name} did not stop after the ledger did not register it: {}", error.user_message());
        }
    }
}
