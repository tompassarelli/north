use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout};
use super::super::pin;
use super::super::remote;
use super::{Flight, PINNED_LOCK_WAIT, PROMOTE_LOCK, advance, built, checkout, follow_main, named_lock, named_lock_waiting, promotion_waiting, usable};
use crate::land_offload;
use rustix::fs::FlockOperation;
use std::fs::{self, File};
use std::time::{Duration, Instant};

const ORDER_LOCK: &str = "runtime-order.lock";
const ORDER_HANDOVER: Duration = Duration::from_secs(10);

/// An order stops the unit that holds runtime-promote.lock before it can take that lock itself, so a promotion that took it in between leaves it to the order runtime-order.lock names (north#1171).
pub(super) fn unordered(layout: &Layout, after_flip: &mut dyn FnMut()) -> Result<Flight, Failure> {
    if named_lock(layout, ORDER_LOCK, FlockOperation::NonBlockingLockShared)?.is_none() {
        println!("north runtime: an order is promoting its commit here, so this promotion leaves it the promotion lock; the next tick takes origin/main's newest head");
        return Ok(Flight::Joined);
    }
    follow_main(layout, after_flip).map(|()| Flight::Flew)
}

/// An order's go-live on a host that takes shipped runtimes, run by the runtime live there: a ship never moves a host back, since a late one must not undo a newer promotion, but an order moves the host to its commit whichever runtime is live. The move is a promotion like any other, so a runtime the hold-back list holds here stays held.
pub(in crate::runtime) fn update_ordered_to(layout: &Layout, target: &str) -> Result<(), Failure> {
    if !land_offload::accepts_peer_runtimes_here() {
        return Err(failed(1, format!("this host settles the fleet and takes an order only as `north runtime order`, not {} shipped from another host", layout::short(target))));
    }
    let _promoting = promotion_waiting(layout, remote::limit(PINNED_LOCK_WAIT), "north runtime update --sha --ordered")?;
    if !usable(&layout.runtime(target)) {
        return Err(failed(1, format!("{} was not shipped to this host; runtime unchanged", layout::short(target))));
    }
    move_to_ordered(layout, target)
}

/// An ordered promotion's go-live on this host: the runtime was built and passed its canary before this, so only the swap is left, and a pass that left `current` elsewhere (held back, rolled back, or moved) is a failure, not a quiet success.
pub(in crate::runtime) fn go_live_ordered(layout: &Layout, target: &str) -> Result<(), Failure> {
    move_to_ordered(layout, target)?;
    pin::promoted(layout, target);
    Ok(())
}

fn move_to_ordered(layout: &Layout, target: &str) -> Result<(), Failure> {
    let checkout = checkout();
    let previous = fs::read_link(layout.current()).ok();
    if previous.as_deref() != Some(layout::link(target).as_path()) {
        advance(layout, &checkout, target, previous.as_deref())?;
        if layout.live_sha().as_deref() != Some(target) {
            return Err(failed(1, format!("{} did not go live here: current is {}", layout::short(target), layout.live_sha().as_deref().map_or("missing", layout::short))));
        }
    }
    Ok(())
}

pub(in crate::runtime) fn build_ordered(layout: &Layout, target: &str) -> Result<(), Failure> {
    built(layout, &checkout(), target).map(|_| ())
}

pub(in crate::runtime) fn order_marked(layout: &Layout) -> Result<File, Failure> {
    named_lock_waiting(layout, ORDER_LOCK, remote::limit(PINNED_LOCK_WAIT), "north runtime order")
}

/// The hold unit blocks on runtime-promote.lock and takes it once the order lets go; the order keeps its mark until it sees the lock held again, since a promotion polling for the lock can reach it first.
pub(in crate::runtime) fn order_handed_over(layout: &Layout) {
    let started = Instant::now();
    while started.elapsed() < ORDER_HANDOVER && !matches!(named_lock(layout, PROMOTE_LOCK, FlockOperation::NonBlockingLockExclusive), Ok(None)) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(in crate::runtime) fn order_waiting(layout: &Layout) -> Result<File, Failure> {
    promotion_waiting(layout, remote::limit(PINNED_LOCK_WAIT), "north runtime order")
}
