//! Runs a heavy build this host would defer on another host with room: the lane's commit and uncommitted diff go to a per-lane checkout there, the command runs under that host's own admission, and its output and exit code come back.
use std::time::Duration;

use crate::capacity::admission::offload_room;
use crate::capacity::build_offload::{Candidate, Declined, Job, PROBE_LIMIT_SECONDS, Room, Route, refusal, route, wants_offload};
use crate::land_offload::{build_hosts, room};

mod claims;
mod lane;
mod remote;
mod script;
#[cfg(test)]
mod tests;

fn room_of(ssh: &str, claim: Option<&claims::Claim>) -> Room {
    let Some(read) = room::read_within(ssh, Duration::from_secs(PROBE_LIMIT_SECONDS)) else { return Room::Unreachable };
    let reserved = claim.map_or(0.0, |claim| claim.ahead() as f64 * offload_room().0);
    room::build_shortfall_reserving(&read, reserved).map_or(Room::Free { free_cpus: read.admission.free_cpus() - reserved }, Room::Short)
}

/// Ok(exit code) when the build ran on another host, trying each host with room in turn when one cannot take the lane; Err leaves it to the local path, naming why when an offload host was configured.
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
    let mut rooms: Vec<(String, Room)> = hosts.iter().zip(&held).map(|((name, ssh), claim)| (name.clone(), room_of(ssh, claim.as_ref()))).collect();
    let mut failed = Vec::new();
    loop {
        let host = match route(candidate, || rooms.clone()) {
            Route::Offload { host } => host,
            Route::Declined(_) if !failed.is_empty() => return Err(Some(Declined::Failed(failed.join("; ")))),
            Route::Declined(declined) => return Err(Some(declined)),
            Route::Local => return Err(None),
        };
        let position = hosts.iter().position(|(name, _)| *name == host).ok_or(None)?;
        let _claim = held[position].take();
        held.iter_mut().for_each(|other| drop(other.take()));
        match run_on(&host, hosts[position].1.as_str(), job, announce)? {
            remote::Ran::Exit(code) => return Ok(code),
            remote::Ran::NotSynced(why) => {
                failed.push(format!("{host}: {why}"));
                rooms[position].1 = Room::Short(why);
            }
        }
    }
}

fn run_on(host: &str, ssh: &str, job: &Job, announce: &dyn Fn(&str)) -> Result<remote::Ran, Option<Declined>> {
    let directory = std::env::current_dir().map_err(|error| Some(Declined::Failed(error.to_string())))?;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).ok_or_else(|| Some(Declined::Failed("HOME is unset".to_owned())))?;
    let plan = script::plan(job.command, &directory, &home).map_err(Some)?;
    let lane = lane::capture(&plan.directory, &plan.command).map_err(Some)?;
    let sink = plan.sink.as_ref().map(script::Sink::open).transpose().map_err(|error| Some(Declined::Failed(format!("the redirect target: {error}"))))?;
    Ok(remote::run(ssh, &lane, &Job { command: &plan.command, ..*job }, sink, &|| announce(host)))
}
