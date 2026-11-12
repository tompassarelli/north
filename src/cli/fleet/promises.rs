use north_dispatch::laptop::Zone;

use crate::fleet::model::Delivery;
use crate::fleet::promises::{Book, Promise, When};
use crate::fleet::registry::{Fleet, root_here};
use crate::fleet::sender::Sender;
use crate::foundation::clock::Timestamp;

pub(super) fn watch(fleet: &Fleet, now_ms: u64) -> Vec<String> {
    let Ok(root) = root_here() else { return Vec::new() };
    let book = Book::at(&root);
    let open = book.open();
    if open.is_empty() {
        return Vec::new();
    }
    let zone = Zone::laptop();
    let (settled, due): (Vec<Promise>, Vec<Promise>) = open.into_iter().filter(|promise| due_ms(promise, &zone).is_none_or(|due| due <= now_ms)).partition(|promise| due_ms(promise, &zone).is_none());
    let keys: Vec<String> = settled.iter().chain(&due).map(Promise::key).collect();
    if let Err(error) = book.checked(&keys) {
        return vec![format!("{} due promises were left for the next tick: their notice could not be recorded, so sending it now could send it twice: {}", due.len(), error.user_message())];
    }
    due.iter().map(|promise| remind(promise, due_ms(promise, &zone).unwrap_or(now_ms), &|parent, text| tell(fleet, parent, text))).collect()
}

fn tell(fleet: &Fleet, parent: &str, text: &str) -> Result<(), String> {
    let from = Sender::fleet();
    if let crate::fleet::registry::Sent::Flooded { line } = fleet.send_message(&from, parent, text, Delivery::Now).map_err(|error| error.user_message())? {
        super::page_flood::record(&from, &line);
    }
    Ok(())
}

pub(super) fn due_ms(promise: &Promise, zone: &Zone) -> Option<u64> {
    let due = match promise.when {
        When::Utc { at_ms } => at_ms,
        When::Wall { hour, minute, date } => {
            let said = i64::try_from(promise.said_ms / 1000).ok()?;
            let at = match date {
                Some((month, day)) => zone.dated(said, month, day, hour.into(), minute.into())?,
                None => zone.nearest(said, hour.into(), minute.into())?,
            };
            u64::try_from(at).ok()? * 1000
        }
    };
    (due > promise.said_ms).then_some(due)
}

pub(super) fn remind(promise: &Promise, due_ms: u64, tell: &dyn Fn(&str, &str) -> Result<(), String>) -> String {
    let at = Timestamp::from_unix_millis(i64::try_from(due_ms).unwrap_or(i64::MAX)).iso_minute();
    let text = format!("promise due: {} reported {:?} in turn {}; its time {at} has come, so check `{}` yourself", promise.session, promise.line, promise.turn, promise.command);
    match tell(&promise.parent, &text) {
        Ok(()) => format!("{text} ({} told)", promise.parent),
        Err(why) => format!("{text} (not told to {}: {why})", promise.parent),
    }
}

#[cfg(test)]
#[path = "promises_tests.rs"]
mod tests;
