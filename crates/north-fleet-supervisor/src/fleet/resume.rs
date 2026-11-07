use std::time::UNIX_EPOCH;

use north_base::platform::attribution::Cause;

use crate::error::NorthResult;
use crate::fleet::model::{Delivery, Phase, SessionRecord, Status};
use crate::fleet::registry::Fleet;
use crate::fleet::sender::Sender;
use crate::fleet::transcript::last_event_at;

pub use north_resume::{Admission, Held, Landing, OPENERS, REVIVED, Settlement, WAITS, Waking, landing};

pub fn ended_ms(record: &SessionRecord) -> Option<u64> {
    let ended = last_event_at(record)?.duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(ended.as_millis()).ok()
}

pub trait Waker {
    fn revive(&self, name: &str) -> NorthResult<()>;
    fn attribute(&self, name: &str, cause: Cause, reason: &str);
}

pub fn resume(fleet: &Fleet, held: &Held, waker: &dyn Waker) -> Vec<String> {
    let mut said = Vec::new();
    for name in held.lanes.keys().chain(held.admissions.keys()).chain(held.settlements.keys()).collect::<std::collections::BTreeSet<_>>() {
        let Ok(record) = fleet.session(name) else { continue };
        let status = fleet.status(&record);
        if record.phase() == Some(Phase::Stopped) || !matches!(status, Status::Idle | Status::Parked | Status::Exited) {
            continue;
        }
        let Some(ended_ms) = ended_ms(&record) else { continue };
        let Some((waking, text)) = held.waking(name, ended_ms) else { continue };
        if fleet.resumed_since_report(&record) {
            continue;
        }
        let woken = (if status == Status::Exited { waker.revive(name) } else { Ok(()) }).and_then(|()| fleet.send_signed(&Sender::fleet(), name, &text, Delivery::Queued));
        match woken {
            Ok(()) => {
                waker.attribute(name, waking.cause(), waking.reason());
                said.push(format!("resumed {name}: {}", waking.said()));
            }
            Err(error) => said.push(format!("not resumed {name}: {}", error.user_message())),
        }
    }
    said
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
