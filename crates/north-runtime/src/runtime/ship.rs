use north_foundation::foundation::failure::Failure;
use super::layout::Layout;
use super::pin::{Event, note, now};
use super::review_hold::Screen;
use north_base::platform::host;
use north_offload::land_offload::{self, Deployment};

pub(super) fn live_here(layout: &Layout) -> Result<String, String> {
    layout.live_sha().ok_or_else(|| "this host has no live runtime".to_owned())
}

/// Ships the live runtime to each other deploy host that no standing hold names, or nothing when `screen` refuses it live on this host (a reviewer's hold naming this host, or an after-landing run not ended green); a trip found after its promotion ships it all the same (Screen::live). A host a hold names is left out as `order --except` leaves it out: recorded in runtime-pins.jsonl and printed with its hold.
pub(super) fn ship_screened<T>(layout: &Layout, live: Result<String, String>, screen: &Screen, ship: impl FnOnce(&str, &[String]) -> Result<T, Failure>) -> Result<Option<T>, Failure> {
    let here = host::name();
    let held_at = north_worker_sandbox::hold_ledger::now();
    match live.and_then(|sha| screen.live(&sha, &[here.as_str()], held_at).map(|_| sha)) {
        Ok(sha) => {
            let (kept, left_out) = held_out(&land_offload::deployment(), screen, &sha, held_at);
            for (destination, why) in &left_out {
                println!("north runtime: not shipped to {destination}: {why}");
                note(layout, &Event::LeftOut { sha: sha.clone(), host: destination.clone(), orderer: here.clone(), reason: why.clone(), at: now() });
            }
            if kept.is_empty() && !left_out.is_empty() {
                return Ok(None);
            }
            ship(&sha, &kept).map(Some)
        }
        Err(why) => {
            println!("north runtime: not shipped to the other hosts: {why}");
            Ok(None)
        }
    }
}

/// The destinations of the deploy hosts `screen` lets `sha` ship to, and those a standing hold names with its refusal. The settling host's own screen has already refused every hold naming no host, so only a hold naming a deploy host leaves it out.
fn held_out(plan: &Deployment, screen: &Screen, sha: &str, at: u64) -> (Vec<String>, Vec<(String, String)>) {
    let mut kept = Vec::new();
    let mut left_out = Vec::new();
    for (name, destination) in plan.names.iter().zip(&plan.destinations) {
        match screen.live(sha, &[name.as_str()], at) {
            Ok(_) => kept.push(destination.clone()),
            Err(why) => left_out.push((destination.clone(), why)),
        }
    }
    (kept, left_out)
}

#[cfg(test)]
mod tests;
