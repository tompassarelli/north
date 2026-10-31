use crate::error::NorthResult;
use crate::fleet::relay::over_ssh;

use super::FleetRow;

pub fn take_from(host: &str) -> NorthResult<Vec<FleetRow>> {
    take_with(|words| over_ssh(host, None, words))
}

pub fn drop_on(host: &str, ids: &[String]) -> NorthResult<()> {
    drop_with(|words| over_ssh(host, None, words), ids)
}

pub fn take_with(run: impl Fn(&[&str]) -> NorthResult<Vec<u8>>) -> NorthResult<Vec<FleetRow>> {
    Ok(serde_json::from_slice(&run(&["fleet", "spool", "take"])?)?)
}

pub fn drop_with(run: impl Fn(&[&str]) -> NorthResult<Vec<u8>>, ids: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = ["fleet", "spool", "drop"].into_iter().chain(ids.iter().map(String::as_str)).collect();
    run(&words).map(|_| ())
}
