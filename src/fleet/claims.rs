//! The threads claim a fleet session holds on its task while its host runs, so a hand launch is as visible to dispatch as a dispatched one.
use std::collections::BTreeSet;

use crate::error::{NorthError, NorthResult};

use super::model::SessionRecord;

pub trait Claims: Send + Sync {
    /// Claims `item` for `session`; a claim a live owner holds is `NorthError::Refused`, while one an exited owner left or one past its ETA moves to `session`, and Some says from whom and why.
    fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>>;
    /// Gives the claim up only while `session` still owns it, so a later owner's claim stays.
    fn release(&self, item: &str, session: &str) -> NorthResult<()>;
    /// Every claim row as (item, owner), including those no dispatch or launch honours any more.
    fn holders(&self) -> NorthResult<Vec<(String, String)>>;
}

/// Claims every task before the session starts, so a live holder refuses the launch and nothing is left half-claimed; a ledger that cannot be read waits for the claim `hold_for` makes once the session runs.
pub fn hold_all(claims: &dyn Claims, tasks: &[String], session: &str) -> NorthResult<()> {
    for (index, task) in tasks.iter().enumerate() {
        match claims.hold(task, session) {
            Err(error @ NorthError::Refused(_)) => {
                for held in &tasks[..index] {
                    let _ = claims.release(held, session);
                }
                return Err(error);
            }
            Err(error) => eprintln!("north fleet: {session} does not hold {task} yet: {}", error.user_message()),
            Ok(Some(moved)) => eprintln!("north fleet: {moved}"),
            Ok(None) => {}
        }
    }
    Ok(())
}

pub fn hold_for(claims: &dyn Claims, record: &SessionRecord) {
    if record.verification {
        return;
    }
    for task in record.tasks() {
        if let Err(error) = claims.hold(task, &record.name) {
            eprintln!("north fleet: {} does not hold {task}: {}", record.name, error.user_message());
        }
    }
}

/// Gives up every claim whose owner is in `exited`, whatever its standing: a session that exited holds nothing, and the ledger keeps its row until someone clears it.
pub fn release_exited(claims: &dyn Claims, exited: &BTreeSet<String>) -> NorthResult<usize> {
    let mut released = 0;
    for (item, owner) in claims.holders()? {
        if !exited.contains(&owner) {
            continue;
        }
        match claims.release(&item, &owner) {
            Ok(()) => released += 1,
            Err(error) => eprintln!("north fleet: {owner} did not release {item}: {}", error.user_message()),
        }
    }
    Ok(released)
}

pub fn release_for(claims: &dyn Claims, record: &SessionRecord) {
    if record.verification {
        return;
    }
    for task in record.tasks() {
        if let Err(error) = claims.release(task, &record.name) {
            eprintln!("north fleet: {} did not release {task}: {}", record.name, error.user_message());
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct Held(pub std::sync::Mutex<Vec<(&'static str, String, String)>>);

#[cfg(test)]
impl Claims for Held {
    fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
        self.0.lock().unwrap().push(("hold", item.to_owned(), session.to_owned()));
        Ok(None)
    }

    fn release(&self, item: &str, session: &str) -> NorthResult<()> {
        self.0.lock().unwrap().push(("release", item.to_owned(), session.to_owned()));
        Ok(())
    }

    fn holders(&self) -> NorthResult<Vec<(String, String)>> {
        let mut held: Vec<(String, String)> = Vec::new();
        for (kind, item, session) in self.0.lock().unwrap().iter() {
            if *kind == "hold" {
                held.retain(|(held, _)| held != item);
                held.push((item.clone(), session.clone()));
            } else {
                held.retain(|(held, owner)| !(held == item && owner == session));
            }
        }
        Ok(held)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Owned<'a>(&'a str, Mutex<Vec<String>>);

    impl Claims for Owned<'_> {
        fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
            if item == self.0 {
                return Err(NorthError::Refused(format!("{item} is already owned by tom")));
            }
            self.1.lock().unwrap().push(format!("hold {item} {session}"));
            Ok(None)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.1.lock().unwrap().push(format!("release {item} {session}"));
            Ok(())
        }

        fn holders(&self) -> NorthResult<Vec<(String, String)>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn a_launch_naming_an_item_a_live_session_holds_is_refused_and_leaves_no_claim_behind() {
        let claims = Owned("north#9", Mutex::default());
        let tasks = ["north#7".to_owned(), "north#9".to_owned()];
        let error = hold_all(&claims, &tasks, "worker").unwrap_err();
        assert!(matches!(error, NorthError::Refused(why) if why == "north#9 is already owned by tom"));
        assert_eq!(*claims.1.lock().unwrap(), ["hold north#7 worker", "release north#7 worker"]);
    }

    #[test]
    fn every_item_a_launch_names_is_claimed_before_it_starts() {
        let claims = Owned("none#0", Mutex::default());
        hold_all(&claims, &["north#7".to_owned(), "north#8".to_owned()], "worker").unwrap();
        assert_eq!(*claims.1.lock().unwrap(), ["hold north#7 worker", "hold north#8 worker"]);
    }
}
