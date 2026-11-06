use std::collections::BTreeMap;

use serde_json::json;

use super::unleased_native::unleased_native;

const NATIVE: &str = "/user.slice/user-1000.slice/user@1000.service/native.slice";

// KEEP(bug): a Mak'gora native window outside any lease is named in status, so a quiet latency holder sees why its frame rate fell (whiterabbit, 2026-11-05)
#[test]
fn a_native_window_outside_every_lease_is_named_with_its_unit_and_fleet_owner_and_a_leased_one_is_not() {
    let leases = [json!({ "id": "21de55e3-8f84-435a-93cc-35526efc7160", "owner": "flash-gap-244", "class": "native" })];
    let clients = [
        json!({ "pid": 11, "prefix": "prefix:lan0a", "cgroup": format!("{NATIVE}/agent-capacity-21de55e38f84435a93cc35526efc7160.scope") }),
        json!({ "pid": 12, "prefix": "prefix:part9a", "cgroup": "/user.slice/user-1000.slice/user@1000.service/agent.slice/north-fleet-part9a-1122-0f1e.service" }),
        json!({ "pid": 13, "prefix": null, "cgroup": "/user.slice/user-1000.slice/session-3.scope" }),
    ];
    let units = BTreeMap::from([("north-fleet-part9a-1122-0f1e.service".to_owned(), "part9a-1122".to_owned())]);
    let unleased = unleased_native(&clients, &leases, &units);
    let named: Vec<_> = unleased.iter().map(|window| (window["pid"].as_u64().unwrap(), window["unit"].as_str().unwrap(), window["owner"].as_str())).collect();
    assert_eq!(named, [(12, "north-fleet-part9a-1122-0f1e.service", Some("part9a-1122")), (13, "session-3.scope", None)]);
}
