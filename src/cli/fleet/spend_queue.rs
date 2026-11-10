use crate::error::NorthResult;
use crate::fleet::launch_queue::{Priority, QueuedLaunch};
use crate::fleet::model::LaunchRequest;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;

use super::placement::Overrides;

pub(super) struct Refused<'a> {
    pub request: &'a LaunchRequest,
    pub overrides: &'a Overrides,
    pub take_over: bool,
    pub priority: Priority,
    pub caller: Option<String>,
    pub words: &'a [String],
    pub why: String,
}

pub(super) fn enqueue(fleet: &Fleet, refused: Refused<'_>) -> NorthResult<()> {
    let mut entry = QueuedLaunch::new(refused.request.clone(), refused.priority, now_ms(), refused.why);
    entry.words = refused.words.to_vec();
    (entry.local, entry.pin, entry.tier, entry.take_over, entry.caller) = (refused.overrides.local.clone(), refused.overrides.pin.clone(), refused.overrides.tier.clone(), refused.take_over, refused.caller);
    fleet.queue_launch(&entry)
}
