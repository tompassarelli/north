use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;

use super::admission;
use super::launch::LaunchParts;
use super::placement::Overrides;
use super::router;

const RETRY_MS: u64 = 3 * 60 * 1000;
const BUDGET: Duration = Duration::from_secs(15);

pub(super) fn drain(fleet: &Fleet) -> NorthResult<Vec<String>> {
    let started = Instant::now();
    let now = now_ms();
    let mut lines = Vec::new();
    for queued in fleet.queued_launches()? {
        if queued.tried_ms.is_some_and(|tried| now.saturating_sub(tried) < RETRY_MS) {
            continue;
        }
        fleet.mark_queued_tried(&queued.name, now)?;
        let overrides = Overrides { local: queued.local.clone(), pin: queued.pin.clone(), tier: queued.tier.clone() };
        let parts = LaunchParts { request: queued.request.clone(), overrides, wait: None, take_over: queued.take_over, priority: queued.priority };
        match router::launch_prepared(fleet, parts, queued.caller.clone(), Some(&queued.name)) {
            Ok(()) => lines.push(format!("started queued launch {} at {} priority", queued.name, queued.priority.label())),
            Err(NorthError::Saturated(why)) => lines.push(format!("queued launch {} still waits: {why}", queued.name)),
            Err(error) => {
                fleet.remove_queued(&queued.name)?;
                admission::tell_parent(fleet, queued.request.parent.as_deref(), &format!("[from {} · queued launch dropped] Not done: the queued launch was dropped: {}\nStop: reasoning", queued.name, error.user_message()));
                lines.push(format!("dropped queued launch {}: {}", queued.name, error.user_message()));
            }
        }
        if started.elapsed() >= BUDGET {
            break;
        }
    }
    Ok(lines)
}
