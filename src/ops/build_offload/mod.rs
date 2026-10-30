//! Runs a heavy build this host would defer on another host with room: the lane's commit and uncommitted diff go to a per-lane checkout there, the command runs under that host's own admission, and its output and exit code come back.
use std::time::Duration;

use crate::capacity::admission::offload_room;
use crate::capacity::build_offload::{Candidate, Job, PROBE_LIMIT_SECONDS, Room, route, wants_offload};
use crate::ops::land_offload::{build_hosts, room};

mod claims;
mod lane;
mod remote;

fn room_of(ssh: &str, claim: Option<&claims::Claim>) -> Room {
    let Some(read) = room::read_within(ssh, Duration::from_secs(PROBE_LIMIT_SECONDS)) else { return Room::Unreachable };
    let reserved = claim.map_or(0.0, |claim| claim.ahead() as f64 * offload_room().0);
    room::shortfall_reserving(&read, reserved).map_or(Room::Free, Room::Short)
}

/// Some(exit code) when the build ran on another host; None leaves it to the local path.
pub(crate) fn offload(candidate: &Candidate, job: &Job, announce: &dyn Fn(&str)) -> Option<u8> {
    if !wants_offload(candidate) {
        return None;
    }
    let hosts = build_hosts();
    let mut held: Vec<Option<claims::Claim>> = hosts.iter().map(|(name, _)| claims::Claim::take(name)).collect();
    let chosen = route(candidate, || hosts.iter().zip(&held).map(|((name, ssh), claim)| (name.clone(), room_of(ssh, claim.as_ref()))).collect());
    let crate::capacity::build_offload::Route::Offload { host } = chosen else { return None };
    let position = hosts.iter().position(|(name, _)| *name == host)?;
    let _claim = held.swap_remove(position);
    drop(held);
    let ssh = hosts.iter().find(|(name, _)| *name == host).map(|(_, ssh)| ssh.as_str())?;
    let lane = match std::env::current_dir().map_err(|error| error.to_string()).and_then(|directory| lane::capture(&directory)) {
        Ok(lane) => lane,
        Err(why) => {
            eprintln!("north capacity offload: {why}");
            return None;
        }
    };
    match remote::run(ssh, &lane, job, &|| announce(&host)) {
        remote::Ran::Exit(code) => Some(code),
        remote::Ran::NotSynced(why) => {
            eprintln!("north capacity offload: {host}: {why}");
            None
        }
    }
}
