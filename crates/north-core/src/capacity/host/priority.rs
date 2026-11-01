//! Who may take the front of the queue: the tom class's own owners, and the tom-ask priority any owner sets for work Tom
//! is waiting on, which places its ticket first within its class and still waits on CPU, memory and pressure.

use serde_json::Value;

use super::json::{Obj, field, text};
use super::policy::{default_number, default_strings};
use super::request::{Request, is_run};

pub const TOM_ASK: &str = "tom-ask";
// Ticket names begin with a 15-digit arrival time, so this sorts after every queued ticket.
const UNQUEUED: &str = "~";

/// Why `owner` may not run in the tom class, or None for one of the play path's own owners (harness/defaults.toml `tom_class_owners`).
pub fn tom_class_refusal(owner: Option<&str>) -> Option<String> {
    let owners = default_strings("tom_class_owners");
    if owner.is_some_and(|owner| owners.iter().any(|allowed| allowed == owner)) {
        return None;
    }
    Some(format!(
        "{} may not use --class tom: it admits past the queue and pressure, so only the play path's own builds ({}) use it; use heavy for builds and tests, gpu for renders or native --latency for frame-timed play, and add --priority tom-ask when Tom is waiting on the work",
        owner.map_or_else(|| "a run with no owner".to_owned(), |owner| format!("owner {owner}")),
        owners.join(", "),
    ))
}

/// Why `class` takes no priority, or None for a class whose tickets wait in a line it can place first in.
pub fn priority_refusal(priority: &str, class: &str) -> Option<String> {
    if priority != TOM_ASK {
        return Some(format!("--priority must be {TOM_ASK}"));
    }
    match class {
        "moderate" | "heavy" | "gpu" => None,
        "critical" | "tom" | "runtime" => Some(format!("--priority {TOM_ASK} places a moderate, heavy or gpu ticket; {class} is already admitted at the front of the queue")),
        _ => Some(format!("--priority {TOM_ASK} places a moderate, heavy or gpu ticket, not {class}")),
    }
}

fn priority_lease(lease: &Value) -> bool {
    is_run(lease) && text(lease, "priority") == Some(TOM_ASK)
}

/// Whether a priority still places a ticket first: fewer than `priority_lease_cap` running leases on this host were admitted with one.
pub fn open(active: &[Value]) -> bool {
    (active.iter().filter(|lease| priority_lease(lease)).count() as f64) < default_number("priority_lease_cap")
}

/// A ticket's place in line: arrival order, except that a placed priority ticket takes the place of the oldest ticket of its class.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Place<'a> {
    at: &'a str,
    unplaced: bool,
    name: &'a str,
}

fn name(entry: &Value) -> &str {
    text(entry, "name").unwrap_or_default()
}

fn placed(entry: &Value) -> bool {
    text(entry, "priority") == Some(TOM_ASK)
}

fn place<'a>(queue: &'a [Value], class: &str, arrival: &'a str, priority: bool) -> Place<'a> {
    let oldest = || queue.iter().filter(|entry| text(entry, "class") == Some(class)).map(name).filter(|other| *other <= arrival).min();
    let at = if priority { oldest().unwrap_or(arrival) } else { arrival };
    Place { at, unplaced: !priority, name: arrival }
}

/// The place of a queued ticket, given whether priorities are open.
pub fn ticket_place<'a>(queue: &'a [Value], entry: &'a Value, open: bool) -> Place<'a> {
    place(queue, text(entry, "class").unwrap_or_default(), name(entry), open && placed(entry))
}

/// The place of the request deciding now, by its ticket or, before it has one, behind every queued ticket.
pub fn request_place<'a>(queue: &'a [Value], request: &'a Request, ticket: Option<&'a str>, open: bool) -> Place<'a> {
    place(queue, request.admitted_class(), ticket.unwrap_or(UNQUEUED), open && request.priority.is_some())
}

/// The queue in admission order for `status`: critical, tom and runtime, then exclusive, then the rest by place.
pub fn admission_order(queue: &[Value], open: bool) -> Vec<Value> {
    let rank = |ticket: &Value| match text(ticket, "class") {
        Some("critical" | "tom" | "runtime") => 0,
        Some("exclusive") => 1,
        _ => 2,
    };
    let mut ordered: Vec<&Value> = queue.iter().collect();
    ordered.sort_by(|left, right| rank(left).cmp(&rank(right)).then_with(|| ticket_place(queue, left, open).cmp(&ticket_place(queue, right, open))));
    ordered.into_iter().cloned().collect()
}

/// Records an admitted priority run, placed or not, with its owner and wait in capacity.db, so every use of the lane is visible.
pub fn record_use(owner: &str, request: &Request, admitted: &Obj, command: &[String]) {
    let Some(priority) = request.priority else { return };
    let view = admitted.value();
    let row = super::database::PriorityUse {
        lease: text(&view, "lease").unwrap_or_default(),
        owner,
        class: &request.name,
        priority,
        placed: view.get("priorityPlaced").and_then(Value::as_bool).unwrap_or(false),
        queued_seconds: field(&view, "queuedSeconds").unwrap_or(0.0),
        command: &Value::from(super::shape::recorded_command(command)).to_string(),
    };
    if let Err(error) = super::database::capacity_database().and_then(|db| super::database::insert_priority_use(&db, &row)) {
        eprintln!("machine-capacity: capacity database failed: {error}");
    }
}

#[cfg(test)]
#[path = "priority_tests.rs"]
mod tests;
