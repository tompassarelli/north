//! The threads claim a fleet session holds on its task while its host runs, so a hand launch is as visible to dispatch as a dispatched one.
use crate::error::NorthResult;

use super::model::SessionRecord;

pub trait Claims: Send + Sync {
    fn hold(&self, item: &str, session: &str) -> NorthResult<()>;
    /// Gives the claim up only while `session` still owns it, so a later owner's claim stays.
    fn release(&self, item: &str, session: &str) -> NorthResult<()>;
}

pub fn hold_for(claims: &dyn Claims, record: &SessionRecord) {
    let Some(task) = record.task_ref.as_deref().filter(|_| !record.verification) else { return };
    if let Err(error) = claims.hold(task, &record.name) {
        eprintln!("north fleet: {} does not hold {task}: {}", record.name, error.user_message());
    }
}

pub fn release_for(claims: &dyn Claims, record: &SessionRecord) {
    let Some(task) = record.task_ref.as_deref().filter(|_| !record.verification) else { return };
    if let Err(error) = claims.release(task, &record.name) {
        eprintln!("north fleet: {} did not release {task}: {}", record.name, error.user_message());
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct Held(pub std::sync::Mutex<Vec<(&'static str, String, String)>>);

#[cfg(test)]
impl Claims for Held {
    fn hold(&self, item: &str, session: &str) -> NorthResult<()> {
        self.0.lock().unwrap().push(("hold", item.to_owned(), session.to_owned()));
        Ok(())
    }

    fn release(&self, item: &str, session: &str) -> NorthResult<()> {
        self.0.lock().unwrap().push(("release", item.to_owned(), session.to_owned()));
        Ok(())
    }
}
