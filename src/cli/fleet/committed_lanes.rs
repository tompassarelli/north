//! The fleet tick's pass over committed lanes whose sessions ended without handing them in (north#1036).
use std::path::Path;

use north_lander::land::committed_lane::{found, hand};
use rusqlite::{Connection, OpenFlags};

use crate::fleet::registry::Fleet;
use crate::land::queue;
use crate::platform::process::run_within;
use crate::usage::{ledger_path, profiles};
use crate::watch;

const QUERY: std::time::Duration = std::time::Duration::from_secs(10);

/// Hands each such lane this host's fleet records show ended to `north finish`, one line per lane.
pub(super) fn pass(fleet: &Fleet) -> Vec<String> {
    let Ok(records) = fleet.sessions() else { return Vec::new() };
    let Ok(db) = Connection::open_with_flags(ledger_path(&profiles::home(), None), OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Vec::new() };
    let ended = |name: &str| records.iter().any(|record| record.name == name && record.ended());
    let Ok(north) = std::env::current_exe() else { return Vec::new() };
    found(&db, ended, queued)
        .iter()
        .map(|lane| match hand(lane, &north) {
            Ok(()) => format!("committed lane {} of ended session {} handed to north finish with `{}`", lane.lane.display(), lane.session, lane.check),
            Err(why) => format!("committed lane {} of ended session {} not handed: {why}", lane.lane.display(), lane.session),
        })
        .collect()
}

fn queued(lane: &Path) -> bool {
    let origin = run_within("git", &["-C", &lane.to_string_lossy(), "remote", "get-url", "--push", "origin"], QUERY).filter(|out| out.status.success()).map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    let Some(origin) = origin else { return true };
    queue::pending(&queue::directory(&watch::state_home(), &origin)).map_or(true, |tickets| tickets.iter().any(|(_, ticket)| ticket.root == lane))
}
