use super::{Org, declared, moved_to, open, ran_out, since};
use crate::credit::{Grant, launchable};
use crate::fleet::model::Transcript;
use crate::fleet::runtime::Runtime;
use crate::providers::claude::fleet::Claude;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

const POLICY: &str = "[accounts.claude.api_orgs.proton]\nid = \"p\"\nkeys = { gex45 = [\"/k/p\"] }\n[accounts.claude.api_orgs.gmail]\nid = \"g\"\nkeys = { gex45 = [\"/k/g\"] }\n";

fn grant(usd: f64, expires: &str) -> Grant {
    Grant { usd, expires: expires.to_owned() }
}

// KEEP(bug): an api org whose first grant is spent still ranked by that grant's date, so a later grant was drawn before an untouched sooner one (north#737)
// KEEP(bug): proton ran out at Anthropic at $145.76 of list-price spend on its $200 grant while the pool still opened it, and its sessions failed their turns instead of moving to gmail (north#755)
#[test]
fn the_pool_opens_the_org_whose_soonest_grant_with_credit_left_expires_first() {
    let proton = [grant(199.99, "2026-11-08"), grant(200.0, "2026-12-08")];
    let spent = [("2026-11-06".to_owned(), 150.0), ("2026-11-07".to_owned(), 66.23)];
    let today = "2026-11-07";
    let open_grants: BTreeMap<String, Vec<Grant>> = [("p".to_owned(), launchable(&proton, &spent, today)), ("g".to_owned(), launchable(&[grant(200.0, "2026-11-12")], &[], today))].into();
    let orgs = declared(&POLICY.parse().unwrap(), &open_grants, Path::new("/home"), "gex45");
    let first = |orgs: &[Org]| open(orgs, today, |_| false, |_| true).map(|(org, _)| org.name.clone());
    assert_eq!(first(&orgs), Some("gmail".to_owned()));
    let all_spent: BTreeMap<String, Vec<Grant>> = [("p".to_owned(), Vec::new()), ("g".to_owned(), Vec::new())].into();
    assert_eq!(first(&declared(&POLICY.parse().unwrap(), &all_spent, Path::new("/home"), "gex45")), None);

    let at = |usd: f64| launchable(&proton, &[(today.to_owned(), usd)], today);
    assert_eq!(at(100.0).len(), 2);
    assert_eq!(at(145.76), [grant(200.0, "2026-12-08")]);
    let mut transcript = Transcript::default();
    for frame in [
        json!({"type": "user", "uuid": "u1", "message": {"content": "Item: north#722"}}),
        json!({"type": "assistant", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "Credit balance is too low"}]}}),
        json!({"type": "result", "subtype": "success", "is_error": true, "api_error_status": 400, "result": "Credit balance is too low"}),
    ] {
        Claude.observe(&frame, &mut transcript);
    }
    assert!(ran_out(&transcript.entries));
    assert_eq!(moved_to("api:proton", &orgs, today, |_| false).as_deref(), Some("api:gmail"));
    assert_eq!(moved_to("api:proton", &orgs, today, |org| org.name == "gmail"), None);
}

fn append(stream: &Path, frames: &[serde_json::Value]) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(stream).unwrap();
    for frame in frames {
        writeln!(file, "{frame}").unwrap();
    }
}

fn low_credit_turn(uuid: &str) -> [serde_json::Value; 3] {
    [
        json!({"type": "user", "uuid": uuid, "session_id": "c", "message": {"content": "carry on"}}),
        json!({"type": "assistant", "session_id": "c", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "Credit balance is too low"}]}}),
        json!({"type": "result", "session_id": "c", "subtype": "success", "is_error": true, "api_error_status": 400, "result": "Credit balance is too low"}),
    ]
}

// KEEP(bug): a session moved off an org that ran out of credit had its old low-credit turn read again on the next tick, which marked the new org exhausted while it still had credit; and a message delivered after a failed turn hid the failure, so no move happened (north#755)
#[test]
fn only_the_newest_low_credit_turn_on_the_current_account_counts() {
    let scratch = tempfile::tempdir().unwrap();
    let stream = scratch.path().join("stream.jsonl");
    let ran_out_from = |offset: u64| ran_out(&since(&Claude, &stream, offset, Some("c")).entries);
    append(&stream, &low_credit_turn("gmail"));
    append(&stream, &[json!({"type": "result", "session_id": "other", "subtype": "success", "result": "done"})]);
    assert!(ran_out_from(0));
    let moved = std::fs::metadata(&stream).unwrap().len();
    assert!(!ran_out_from(moved));
    append(&stream, &[json!({"type": "user", "uuid": "report", "session_id": "c", "message": {"content": "[from child] Done: landed"}})]);
    assert!(ran_out_from(0));
    assert!(!ran_out_from(moved));
    append(&stream, &low_credit_turn("apple")[1..]);
    assert!(ran_out_from(moved));
}
