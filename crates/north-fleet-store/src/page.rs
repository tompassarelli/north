//! Which service alarms wake the proxy now, and which wait for `north fleet digest`: Tom's gates, a red
//! main, a runtime rollback, a disk past its watermark, a security finding, a health alarm opening a
//! cause, a land-watch page its follower check let through, and a land-watch page that the remote-execution
//! grid or every hand-in lost its verdicts, whose cleared notice wakes the proxy too. A message from a session,
//! Tom or a landing sender is not an alarm service and passes unchanged.
use crate::sender::Sender;

pub fn wakes(sender: &Sender, text: &str) -> bool {
    let Sender::Service(unit) = sender else { return true };
    let unit = unit.split('@').next().unwrap_or(unit);
    let lower = text.to_lowercase();
    match unit {
        "north-health" => health(&lower),
        "north-land-watch" => red_main(&lower) || lower.contains("not live") || lost_checks(&lower),
        "north-ops-flow" => red_main(&lower) || security(&lower) || disk(&lower),
        "north-runtime" => rollback(&lower),
        "north-capacity-watchdog" => disk(&lower),
        _ => false,
    }
}

pub fn clear_wakes(sender: &Sender, text: &str) -> bool {
    matches!(sender, Sender::Service(unit) if unit.split('@').next() == Some("north-land-watch")) && lost_checks(&text.to_lowercase())
}

fn lost_checks(text: &str) -> bool {
    text.contains("north grid: ") || text.contains("north land: no verdict ")
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
