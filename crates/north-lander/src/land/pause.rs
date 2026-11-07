use std::path::Path;
use std::process::Command;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Record, Store};

use super::after_landing_lifecycle::{self as lifecycle, BaseBroken, Pause};
use north_check_environment::instance;

const ALERT_TITLE: &str = "Lander paused";

fn alert(detail: &str) -> bool {
    Command::new("supply-alert").args(["normal", detail, ALERT_TITLE]).status().is_ok_and(|status| status.success())
}


fn retry_at(record: &Record<Pause>) -> Timestamp {
    Timestamp::from_unix_millis(record.since.unix_millis() + i64::try_from(BaseBroken::RETRY.as_millis()).unwrap_or(i64::MAX))
}

fn waiting(record: &Record<Pause>, now: Timestamp) -> String {
    format!("{}; new candidates are paused and the base check is retried in {} s", record.cause, (retry_at(record).unix_seconds() - now.unix_seconds()).max(0))
}

fn with_store<T>(act: impl FnOnce(&mut Store) -> T) -> Result<T, String> {
    let mut store = lifecycle::host()?;
    Ok(act(&mut store))
}

pub fn held(queue: &Path) -> Option<String> {
    with_store(|store| held_in(store, &instance(queue), Timestamp::now())).unwrap_or_else(|error| {
        println!("{error}");
        None
    })
}

pub fn raise(queue: &Path, detail: &str) -> Result<String, String> {
    with_store(|store| raise_in(store, &instance(queue), detail, Timestamp::now(), alert))?
}

pub fn resume(queue: &Path) {
    if let Err(error) = with_store(|store| resume_in(store, &instance(queue))) {
        println!("{error}");
    }
}

pub(crate) fn held_in(store: &Store, queue: &str, now: Timestamp) -> Option<String> {
    let record = lifecycle::current::<BaseBroken>(store, queue)?;
    (record.state != Pause::Clear && retry_at(&record) > now).then(|| waiting(&record, now))
}

/// Pauses new candidates for `BaseBroken::RETRY`. A pause no raise renewed for two retry windows ended unseen, so a
/// raise after it is a new breakage and alerts again instead of inheriting the old alert.
pub(crate) fn raise_in(store: &mut Store, queue: &str, detail: &str, now: Timestamp, alert: impl Fn(&str) -> bool) -> Result<String, String> {
    let current = lifecycle::current::<BaseBroken>(store, queue);
    let lapsed = current.as_ref().is_some_and(|record| now.unix_millis() > retry_at(record).unix_millis() + i64::try_from(BaseBroken::RETRY.as_millis()).unwrap_or(i64::MAX));
    let renewed = match current.map(|record| record.state) {
        Some(Pause::Alerted) if !lapsed => lifecycle::moved::<BaseBroken>(store, queue, Pause::Alerted, detail),
        _ => lifecycle::moved::<BaseBroken>(store, queue, Pause::Paused, detail).and_then(|paused| {
            if alert(detail) {
                lifecycle::moved::<BaseBroken>(store, queue, Pause::Alerted, detail)
            } else {
                eprintln!("north land: supply-alert failed for: {detail}");
                Some(paused)
            }
        }),
    };
    renewed.map(|record| waiting(&record, now)).ok_or_else(|| format!("north land: the base-broken pause of {queue} could not be recorded: {detail}"))
}

pub(crate) fn resume_in(store: &mut Store, queue: &str) {
    if lifecycle::current::<BaseBroken>(store, queue).is_some_and(|record| record.state != Pause::Clear)
        && lifecycle::moved::<BaseBroken>(store, queue, Pause::Clear, "main's check environment passes again").is_some()
    {
        println!("north land: main's check environment passes again; candidates resume");
    }
}
