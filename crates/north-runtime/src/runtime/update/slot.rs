use super::super::cadence;
use super::super::layout::{self, Layout};
use super::super::pin;
use super::sync::send_alert;
use super::{ahead, ancestor, built, checkout, named_lock, usable};
use crate::land_offload;
use crate::platform::host;
use rustix::fs::FlockOperation;
use std::path::Path;

const PAGE_LOCK: &str = "runtime-page.lock";

pub(super) fn target(layout: &Layout, checkout: &Path, main: &str) -> String {
    let ready = |sha: &str| usable(&layout.runtime(sha)) || ahead::ready_for(layout, checkout, sha).is_some();
    let mut build = |sha: &str| {
        if let Err(failure) = built(layout, checkout, sha) {
            println!("north runtime: {} was not built ahead of its slot and is built at it: {}", layout::short(sha), failure.message);
        }
    };
    cadence::target(layout, checkout, main, &ready, &mut build)
}

/// Pages the coordinator when a promotion that fell due is not live five minutes later (pin::page); only the host that settles the fleet promotes on the schedule, so only it has a due promotion to page about, and the caller holding the page lock is the only one that reads the journal, sends and records.
pub(super) fn page_overdue(layout: &Layout) {
    if land_offload::settles_fleet_here() && let Ok(Some(_paging)) = named_lock(layout, PAGE_LOCK, FlockOperation::NonBlockingLockExclusive) {
        let checkout = checkout();
        pin::page(layout, pin::now(), &host::name(), &mut |live, due| ancestor(&checkout, due, live), &mut |text| send_alert(text));
    }
}
