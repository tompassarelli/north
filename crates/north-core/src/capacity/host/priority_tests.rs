use serde_json::{Value, json};

use super::super::decision::queued_ahead;
use super::super::request::Request;
use super::{TOM_ASK, open, tom_class_refusal};
use crate::capacity::admission::{Class, Facts, Reason, decide};

fn ticket(name: &str, class: &str, priority: bool) -> Value {
    let mut ticket = json!({ "name": name, "class": class, "requestedClass": class });
    if priority {
        ticket["priority"] = json!(TOM_ASK);
    }
    ticket
}

fn moderate(priority: bool) -> Request {
    Request { name: "moderate".to_owned(), priority: priority.then_some(TOM_ASK), ..Request::default() }
}

fn ahead(queue: &[Value], request: &Request, own: &str, open: bool) -> Vec<String> {
    queued_ahead(queue, request, Some(own), open, &|_| false).ahead.iter().map(|entry| entry["name"].as_str().unwrap_or_default().to_owned()).collect()
}

fn run_lease(priority: bool) -> Value {
    let mut lease = json!({ "kind": "run", "class": "heavy" });
    if priority {
        lease["priority"] = json!(TOM_ASK);
    }
    lease
}

fn decided(request: &Request, queued: usize, cpu_some_basis_points: u32) -> Reason {
    let class: Class = serde_json::from_value(json!(request.admitted_class())).expect("class");
    decide(&Facts { queued_ahead: u32::try_from(queued).unwrap(), cpu_some_avg10_basis_points: cpu_some_basis_points, ..crate::capacity::tests::facts(class) })
}

#[test]
fn an_agent_asking_for_the_tom_class_is_refused_and_told_which_class_to_use() {
    for owner in [Some("wisp-fx-style"), Some("perf-scale-185"), Some("land-check"), None] {
        let refusal = tom_class_refusal(owner).unwrap_or_else(|| panic!("{owner:?} admitted to the tom class"));
        assert!(refusal.contains("heavy") && refusal.contains("--priority tom-ask"), "{refusal}");
    }
}

#[test]
fn the_tom_build_watcher_play_helper_and_proxy_preview_are_admitted_to_the_tom_class() {
    for owner in ["smashcraft:tom-build-window", "smashcraft:play-helper", "tom-preview"] {
        assert_eq!(tom_class_refusal(Some(owner)), None, "{owner}");
    }
}

#[test]
fn a_priority_ticket_goes_ahead_of_an_older_ticket_of_its_class_but_still_waits_on_cpu_pressure() {
    let queue = [ticket("000000000000001-a", "moderate", false), ticket("000000000000002-b", "heavy", false), ticket("000000000000003-c", "moderate", true)];
    let asked = moderate(true);
    let placed = ahead(&queue, &asked, "000000000000003-c", true);
    assert!(placed.is_empty(), "the priority ticket waits behind {placed:?}");
    assert_eq!(ahead(&queue, &moderate(false), "000000000000001-a", true), ["000000000000003-c"]);
    assert_eq!(decided(&asked, placed.len(), 0), Reason::Run);
    assert_eq!(decided(&asked, placed.len(), 9000), Reason::DeferCpuPressure);
}

#[test]
fn past_two_running_priority_leases_a_priority_ticket_keeps_its_arrival_place() {
    let queue = [ticket("000000000000001-a", "moderate", false), ticket("000000000000002-b", "moderate", true)];
    let one = [run_lease(true), run_lease(false)];
    let two = [run_lease(true), run_lease(true), run_lease(false)];
    assert!(open(&one));
    assert!(!open(&two));
    assert!(ahead(&queue, &moderate(true), "000000000000002-b", open(&one)).is_empty());
    assert_eq!(ahead(&queue, &moderate(true), "000000000000002-b", open(&two)), ["000000000000001-a"]);
}

