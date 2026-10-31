//! Runs a heavy build this host would defer on another host with room: the lane's commit and uncommitted diff go to a per-lane checkout there, the command runs under that host's own admission, and its output and exit code come back.
use std::time::Duration;

use crate::capacity::admission::offload_room;
use crate::capacity::build_offload::{Candidate, Declined, Job, PROBE_LIMIT_SECONDS, Room, Route, refusal, route, wants_offload};
use crate::ops::land_offload::{build_hosts, room};

mod claims;
mod lane;
mod remote;

fn room_of(ssh: &str, claim: Option<&claims::Claim>) -> Room {
    let Some(read) = room::read_within(ssh, Duration::from_secs(PROBE_LIMIT_SECONDS)) else { return Room::Unreachable };
    let reserved = claim.map_or(0.0, |claim| claim.ahead() as f64 * offload_room().0);
    room::shortfall_reserving(&read, reserved).map_or(Room::Free, Room::Short)
}

/// Ok(exit code) when the build ran on another host; Err leaves it to the local path, naming why when an offload host was configured.
pub fn offload(candidate: &Candidate, job: &Job, announce: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
    if !wants_offload(candidate) {
        return Err(None);
    }
    let hosts = build_hosts();
    if hosts.is_empty() {
        return Err(None);
    }
    if let Some(refused) = refusal(candidate) {
        return Err(Some(refused));
    }
    let mut held: Vec<Option<claims::Claim>> = hosts.iter().map(|(name, _)| claims::Claim::take(name)).collect();
    let host = match route(candidate, || hosts.iter().zip(&held).map(|((name, ssh), claim)| (name.clone(), room_of(ssh, claim.as_ref()))).collect()) {
        Route::Offload { host } => host,
        Route::Declined(declined) => return Err(Some(declined)),
        Route::Local => return Err(None),
    };
    let position = hosts.iter().position(|(name, _)| *name == host).ok_or(None)?;
    let _claim = held.swap_remove(position);
    drop(held);
    let ssh = hosts[position].1.as_str();
    let directory = std::env::current_dir().map_err(|error| Some(Declined::Failed(error.to_string())))?;
    let lane = lane::capture(&directory).map_err(Some)?;
    match remote::run(ssh, &lane, job, &|| announce(&host)) {
        remote::Ran::Exit(code) => Ok(code),
        remote::Ran::NotSynced(why) => Err(Some(Declined::Failed(format!("{host}: {why}")))),
    }
}
