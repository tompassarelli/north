//! The fleet tick's hourly class review (north#1577): once an hour the coordinator is woken with the top harness-loss classes
//! of the last hour, the ones `north work --since 1h` prints, each with the issue that owns it.

use std::path::{Path, PathBuf};

use north_unblocks::review::{ClassLoss, HOUR_SECONDS, Owners, classes, message, mirror_closed, since, threads_db};

use crate::fleet::model::Delivery;
use crate::fleet::registry::Fleet;
use crate::fleet::sender::Sender;

const TOLD: &str = "north/class-review-told";

pub(super) fn tick(fleet: &Fleet, now: i64) -> Vec<String> {
    let read = || since(&threads_db(), now - HOUR_SECONDS).and_then(|reading| classes(&reading, &Owners::here(), &mirror_closed));
    review(&told(), now, read, |text| fleet.send_signed(&Sender::fleet(), "proxy", text, Delivery::Queued).map_err(|error| error.user_message()))
}

fn told() -> PathBuf {
    north_unblocks::state().join(TOLD)
}

/// Sends the coordinator the review of `read`'s classes through `send` when the last review, kept in `told`, is an hour old;
/// an hour with no harness loss is reviewed without a message.
pub(super) fn review(told: &Path, now: i64, read: impl FnOnce() -> Result<Vec<ClassLoss>, String>, send: impl FnOnce(&str) -> Result<(), String>) -> Vec<String> {
    let last = std::fs::read_to_string(told).ok().and_then(|text| text.trim().parse::<i64>().ok());
    if last.is_some_and(|last| now - last < HOUR_SECONDS) {
        return Vec::new();
    }
    let found = match read() {
        Ok(found) => found,
        Err(why) => return vec![format!("class review: the harness losses of the hour are unread: {why}")],
    };
    let mut lines = Vec::new();
    if let Some(text) = message(&found) {
        if let Err(why) = send(&text) {
            return vec![format!("class review: the coordinator was not told: {why}")];
        }
        lines.push(text);
    }
    let kept = told.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(told, format!("{now}\n")));
    if let Err(error) = kept {
        lines.push(format!("class review: {} cannot be written: {error}", told.display()));
    }
    lines
}

#[cfg(test)]
#[path = "class_review_tests.rs"]
mod tests;
