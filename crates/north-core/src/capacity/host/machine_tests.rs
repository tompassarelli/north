use serde_json::json;

use super::machine::latency_scopes;
use super::systemd::scope_unit;

#[test]
fn only_live_native_leases_that_declared_latency_count_as_pressure_on_batch_work() {
    let leases = [
        json!({ "id": "aaaa-1", "class": "native", "owner": "wisp-online-clone-b", "latency": true }),
        json!({ "id": "bbbb-2", "class": "native", "owner": "wisp-lan-pair-4" }),
        json!({ "id": "cccc-3", "class": "native", "owner": "capture-batch", "quiet": true }),
        json!({ "id": "dddd-4", "class": "heavy", "owner": "build", "latency": true }),
    ];
    assert_eq!(latency_scopes(&leases), vec![scope_unit("aaaa-1")]);
    assert!(latency_scopes(&leases[1..3]).is_empty(), "capture, accept and pool clients never hold batch work through pressure");
}
