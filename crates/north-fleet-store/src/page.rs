//! Which service alarms wake the proxy now, and which wait for `north fleet digest`: Tom's gates, a red
//! main, a runtime rollback, a disk past its watermark, a security finding, a health alarm opening a
//! cause, the north lander running too far behind main, and a land-watch page its follower check let through. A message from a session, Tom or a
//! landing sender is not an alarm service and passes unchanged.
use crate::sender::Sender;

pub fn wakes(sender: &Sender, text: &str) -> bool {
    let Sender::Service(unit) = sender else { return true };
    let unit = unit.split('@').next().unwrap_or(unit);
    let lower = text.to_lowercase();
    match unit {
        "north-health" => health(&lower),
        "north-land-watch" => red_main(&lower) || lower.contains("not live"),
        "north-ops-flow" => red_main(&lower) || security(&lower) || disk(&lower),
        "north-runtime" => rollback(&lower),
        "north-capacity-watchdog" => disk(&lower),
        "north-land" => text.starts_with(crate::fold::LANDER_BEHIND),
        _ => false,
    }
}

fn health(text: &str) -> bool {
    text.starts_with("harness unit ")
}

fn red_main(text: &str) -> bool {
    text.contains("red main") || text.contains("red-main") || text.contains("main is red")
}

fn security(text: &str) -> bool {
    text.contains("security finding") || text.contains("security: ")
}

fn disk(text: &str) -> bool {
    text.contains("watermark") || (text.contains("disk") && (text.contains("free") || text.contains("gib")))
}

fn rollback(text: &str) -> bool {
    text.contains("rolled back") || text.contains("rollback")
}
