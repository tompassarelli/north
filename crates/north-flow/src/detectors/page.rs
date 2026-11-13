//! Paging the coordinator and dispatch about an unchecked issue box that no worker names. Each episode is one keyed
//! fleet condition, which the fleet delivers once; the condition clears when the box is staffed or its issue closes.
use std::collections::BTreeSet;

use super::Finding;
use crate::flow::{Host, Places, write_atomic};

const STATE: &str = "paged.json";
const FROM: &str = "service:north-ops-flow";
/// The coordinator is the proxy session, paged urgently; dispatch takes the page in its ordinary queue.
const RECIPIENTS: [(&str, bool); 2] = [("proxy", true), ("dispatch", false)];

fn load(places: &Places) -> BTreeSet<String> {
    std::fs::read_to_string(places.flow().join(STATE)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save(places: &Places, paged: &BTreeSet<String>) -> Result<(), String> {
    write_atomic(&places.flow().join(STATE), &(serde_json::to_string(paged).map_err(|error| error.to_string())? + "\n")).map_err(|error| error.to_string())
}

fn send(host: &dyn Host, session: &str, urgent: bool, key: &str, cleared: bool, text: &str) -> bool {
    let mut argv = vec!["north", "fleet", "send"];
    if urgent {
        argv.push("--now");
    }
    argv.extend(["--from", FROM, "--key", key]);
    if cleared {
        argv.push("--cleared");
    }
    argv.extend([session, text]);
    host.run(&argv, None, None).is_some()
}

/// Sends a page for each new finding and a cleared notice for each episode that ended, and records the episodes
/// still open, so a finding that stays unstaffed pages once.
pub fn deliver(places: &Places, host: &dyn Host, pages: &[Finding]) -> Vec<String> {
    let previous = load(places);
    let current: BTreeSet<String> = pages.iter().map(|finding| finding.key.clone()).collect();
    let mut open = BTreeSet::new();
    let mut lines = Vec::new();
    for finding in pages {
        if previous.contains(&finding.key) {
            open.insert(finding.key.clone());
            continue;
        }
        let text = format!("{} {}", finding.detail, finding.done_when);
        if RECIPIENTS.iter().all(|(session, urgent)| send(host, session, *urgent, &finding.key, false, &text)) {
            open.insert(finding.key.clone());
            lines.push(format!("detectors: {} {}: paged proxy and dispatch", finding.detector.name(), finding.key));
        } else {
            lines.push(format!("detectors: {} {}: page not sent", finding.detector.name(), finding.key));
        }
    }
    for key in previous.difference(&current) {
        let text = format!("{key} is staffed or its issue is closed, so its page is cleared");
        if RECIPIENTS.iter().all(|(session, _)| send(host, session, false, key, true, &text)) {
            lines.push(format!("detectors: {key}: cleared its page"));
        } else {
            open.insert(key.clone());
            lines.push(format!("detectors: {key}: clear not sent, retried next tick"));
        }
    }
    if open != previous && let Err(error) = save(places, &open) {
        lines.push(format!("detectors: paging state not written: {error}"));
    }
    lines
}
