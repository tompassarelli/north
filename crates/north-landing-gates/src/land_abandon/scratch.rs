use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::autoland_status::git_output;

const IDLE_SECONDS: u64 = 86_400;

pub struct ScratchPush {
    pub at_s: u64,
    pub repository: String,
    pub branch: String,
    pub oid: String,
    pub session: String,
}

pub trait Witness {
    fn live_sessions(&self) -> Option<BTreeSet<String>>;
    fn pushes(&self) -> Vec<ScratchPush>;
}

pub(super) struct ScratchUse {
    pub caller: Option<String>,
    pub pusher: Option<String>,
    pub last_active_s: Option<u64>,
    pub now_s: u64,
    pub live: Option<BTreeSet<String>>,
}

pub(super) fn verdict(scratch: &ScratchUse) -> Result<String, String> {
    let named = |session: &Option<String>| session.clone().filter(|session| !session.is_empty());
    let pusher = named(&scratch.pusher);
    if pusher.is_some() && pusher == named(&scratch.caller) {
        return Ok("this session pushed its tip".to_owned());
    }
    if scratch.last_active_s.is_some_and(|active| scratch.now_s.saturating_sub(active) >= IDLE_SECONDS) {
        return Ok("untouched for over a day".to_owned());
    }
    match (pusher, &scratch.live) {
        (Some(pusher), Some(live)) if !live.contains(&pusher) => Ok(format!("{pusher}, which pushed its tip, is no longer running")),
        (Some(pusher), _) => Err(format!("{pusher} pushed it within the day and may still be running")),
        (None, _) => Err("a session this host has no push record of touched it within the day".to_owned()),
    }
}

pub(super) fn observe(branch: &str, tip: &str, witness: &dyn Witness) -> ScratchUse {
    let url = git_output(&["remote", "get-url", "--push", "origin"]).unwrap_or_default();
    let repository = url.trim().rsplit('/').next().unwrap_or_default();
    let repository = repository.strip_suffix(".git").unwrap_or(repository);
    let pushes: Vec<_> = witness
        .pushes()
        .into_iter()
        .filter(|push| push.repository == repository && push.branch == branch)
        .collect();
    let committed_s = git_output(&["log", "-1", "--format=%ct", tip]).and_then(|seconds| seconds.trim().parse::<u64>().ok());
    let pushed_s = pushes.iter().map(|push| push.at_s).max().unwrap_or(0);
    ScratchUse {
        caller: std::env::var("NORTH_FLEET_SESSION").ok(),
        pusher: pushes.iter().rev().find(|push| push.oid == tip).map(|push| push.session.clone()),
        last_active_s: committed_s.map(|committed| committed.max(pushed_s)),
        now_s: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs()),
        live: witness.live_sessions(),
    }
}

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod tests;
