use super::super::cadence;
use super::super::layout::{self, Layout};
use super::super::pin;
use super::super::pin::Outcome;
use super::super::review_hold::{self, Screen};
use super::sync::send_alert;
use super::{ahead, ancestor, built, checkout, named_lock, usable};
use north_offload::land_offload;
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

/// The head this pass promotes. On the host that settles the fleet it is the one the cadence names, unless `screen` finds a reviewer's hold it holds or an after-landing run of it that has not ended green (review_hold.rs). A host that follows origin/main itself while another host settles the fleet, or that cannot read the configuration saying whether one does, takes no head of its own, since only the settling host keeps the holds and the push ledger they are read by, and this host's own would read as missing; with no settling host anywhere no hold can be placed, and the host takes origin/main.
pub(in super::super) fn screened(layout: &Layout, checkout: &Path, main: String, settles: bool, settled_elsewhere: Result<bool, String>, screen: &Screen) -> Option<String> {
    if !settles {
        return unsettled(main, settled_elsewhere);
    }
    let target = target(layout, checkout, &main);
    match held_by_review(layout, screen, &target) {
        Some(why) => {
            println!("north runtime: {why}");
            None
        }
        None => Some(target),
    }
}

/// origin/main's head for a host that does not settle the fleet, unless reviewers' holds are on and another host settles the fleet or the configuration saying so cannot be read.
fn unsettled(main: String, settled_elsewhere: Result<bool, String>) -> Option<String> {
    let why = match settled_elsewhere {
        _ if !review_hold::enabled() => return Some(main),
        Ok(false) => return Some(main),
        Ok(true) => "the reviewers' holds and the push ledger they are read by are kept on the host that settles the fleet".to_owned(),
        Err(error) => format!("its orchestration configuration cannot be read to say which host settles the fleet and keeps the reviewers' holds ({error})"),
    };
    println!("north runtime: {} follows origin/main itself, but {why}, so it takes no head of its own; give it an ssh destination on the settling host so that host ships it the runtime", host::name());
    None
}

/// Why the slot or Live-fix promotion of `target` on the host that settles the fleet stays off, when `screen` finds an after-landing run of it that has not ended green or a reviewer's hold on a commit it contains that the live runtime does not already hold; the refusal is the slot's recorded outcome, so status names it.
pub(in super::super) fn held_by_review(layout: &Layout, screen: &Screen, target: &str) -> Option<String> {
    if layout.live_sha().as_deref() == Some(target) {
        return None;
    }
    let why = format!("{} is not promoted: {}", layout::short(target), screen.promotion(target, review_hold::now()).err()?);
    cadence::note_outcome(layout, pin::now(), Outcome::Refused { reason: why.clone() });
    Some(why)
}

/// Pages the coordinator when a promotion that fell due is not live five minutes later (pin::page); only the host that settles the fleet promotes on the schedule, so only it has a due promotion to page about, and the caller holding the page lock is the only one that reads the journal, sends and records.
pub(super) fn page_overdue(layout: &Layout) {
    if land_offload::settles_fleet_here() && let Ok(Some(_paging)) = named_lock(layout, PAGE_LOCK, FlockOperation::NonBlockingLockExclusive) {
        let checkout = checkout();
        pin::page(layout, pin::now(), &host::name(), &mut |live, due| ancestor(&checkout, due, live), &mut |text| send_alert(text));
    }
}
