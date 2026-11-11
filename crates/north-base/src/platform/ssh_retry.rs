use std::collections::BTreeMap;
use std::ffi::OsString;

use super::ssh::OWN_CONNECTION;
use super::ssh_master;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rerun {
    Anywhere,
    BeforeSend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Shared,
    Fresh,
}

impl Link {
    pub fn options(self) -> &'static [&'static str] {
        match self {
            Self::Shared => &[],
            Self::Fresh => &["-o", OWN_CONNECTION],
        }
    }
}

pub struct Reply<T> {
    pub value: T,
    pub exit: Option<i32>,
    pub said: String,
}

const UNSENT: [&str; 5] = [
    "Connection timed out during banner exchange",
    "ssh: connect to host",
    "kex_exchange_identification",
    "Connection closed by",
    "mux_client_hello_exchange",
];

pub fn retry_reason(rerun: Rerun, exit: Option<i32>, said: &str) -> Option<String> {
    if exit != Some(255) {
        return None;
    }
    let first = said.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("no error text");
    if UNSENT.iter().any(|marker| said.contains(marker)) {
        return Some(format!("ssh exited 255 before the command was sent: {first}"));
    }
    (rerun == Rerun::Anywhere).then(|| format!("ssh exited 255: {first}"))
}

pub fn retried<T>(destination: &str, rerun: Rerun, mut call: impl FnMut(Link) -> Reply<T>) -> T {
    let first = call(Link::Shared);
    let Some(reason) = retry_reason(rerun, first.exit, &first.said) else { return first.value };
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let reopened = ssh_master::reopen_if_dead(&[OsString::from("ssh")], &env, destination);
    let note = reopened.map_or_else(|why| format!("; the master could not be reopened: {why}"), |reopened| if reopened { "; dropped the dead master and asked its unit to reopen it".to_owned() } else { String::new() });
    eprintln!("north ssh: {destination}: attempt 2 of 2 on a fresh connection: {reason}{note}");
    call(Link::Fresh).value
}

#[cfg(test)]
#[path = "ssh_retry_tests.rs"]
mod tests;
