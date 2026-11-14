use crate::error::NorthResult;
use crate::fleet::claims;
use crate::fleet::model::SessionRecord;
use crate::fleet::store::{Claim, SessionRow};

use super::Fleet;

const UNREGISTERED: &str = "the ledger did not register its name";

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

    /// The launch's outcome once its registration is in. A launch that came up unregistered fails with the registration's reason, its session moved off live for the discard that ends its host. A failed launch gives its claims back only when the ledger registered its name: a name the ledger refused may be a live session's on another host, and those claims are that session's.
    pub(super) fn settled(&self, record: &SessionRecord, brought: NorthResult<()>, registered: impl FnOnce() -> NorthResult<()>) -> NorthResult<()> {
        match (brought, registered()) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(error)) => {
                if let Err(left) = self.phases().and_then(|mut phases| phases.stopped(record.launch(), UNREGISTERED)) {
                    eprintln!("north fleet: {} stays recorded live after the ledger did not register it: {}", record.name, left.user_message());
                }
                Err(error)
            }
            (Err(error), Ok(())) => {
                claims::release_for(self.claims.as_ref(), record);
                Err(error)
            }
            (Err(error), Err(_)) => Err(error),
        }
    }
}
