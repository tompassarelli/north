use std::os::fd::OwnedFd;
use std::path::PathBuf;

use super::hold_request::tell;
use super::lane_grant::{self, Host};
use super::peer::Peer;
use crate::runtime::review_fetch::{self, Fetch};

pub(super) fn answer(asked: Result<Fetch, String>, streams: Vec<OwnedFd>, peer: &Peer, host: &Host) -> Result<u8, String> {
    tell(asked.and_then(|fetch| served(&fetch, peer, host)), streams, "a review fetch")
}

fn served(fetch: &Fetch, peer: &Peer, host: &Host) -> Result<String, String> {
    let (_, record) = lane_grant::own_session(peer, host, "a review fetch is made for a fleet session, and this caller runs in no session's scope")?;
    let main = record["cwd"].as_str().map(PathBuf::from).filter(|main| main.is_absolute() && main.file_name().is_some_and(|name| name == "main")).ok_or_else(|| "the caller's session record names no main checkout to fetch a lane for".to_owned())?;
    review_fetch::serve(&main, fetch)
}
