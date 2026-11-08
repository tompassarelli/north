use std::path::Path;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Record, Store};

use super::after_landing_lifecycle::{self as lifecycle, BaseBroken, Pause};
use north_check_environment::instance;

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

fn held_in(store: &Store, queue: &str, now: Timestamp) -> Option<String> {
    let record = lifecycle::current::<BaseBroken>(store, queue)?;
    (record.state != Pause::Clear && retry_at(&record) > now).then(|| waiting(&record, now))
}

