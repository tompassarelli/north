use super::sampling;
use serde_json::json;

#[test]
fn failed_reads_retry_within_ten_minutes_and_leave_workers_unknown_while_read_fleet_lists_sum_every_host() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = json!({"providers": {"claude": {"interval_s": 30720.0, "sampled_at": 1000.0}}});
    let failed = sampling::Reads::from([("claude", (Vec::new(), json!({"code": 2, "elapsed_s": 0.02, "throttled": false, "error": "north usage --rows claude: boom"})))]);
    sampling::record_cadence(directory.path(), &mut state, &failed, 5000.0).unwrap();
    let claude = &state["providers"]["claude"];
    assert_eq!((claude["code"].clone(), claude["interval_s"].clone(), claude["sampled_at"].clone(), claude["next_at"].clone()), (json!(2), json!(600.0), json!(1000.0), json!(5600.0)));
    assert_eq!(claude["error"], "north usage --rows claude: boom");
    let read = sampling::Reads::from([("claude", (Vec::new(), json!({"code": 0, "elapsed_s": 1.0, "throttled": false})))]);
    sampling::record_cadence(directory.path(), &mut state, &read, 5600.0).unwrap();
    let claude = &state["providers"]["claude"];
    assert_eq!((claude["interval_s"].clone(), claude["sampled_at"].clone(), claude.get("error")), (json!(60.0), json!(5600.0), None));

    let session = |name: &str, status: &str, account: &str| json!({"name": name, "status": status, "provider": "claude", "account": account});
    let fleet = vec![
        ("whiterabbit".to_owned(), json!([session("north-1", "working", "a"), session("north-2", "working", "a"), session("north-3", "idle", "a")])),
        ("gex45".to_owned(), json!([session("north-4", "working", "a"), session("north-5", "working", "b")])),
    ];
    let workers = |fleet: Option<&[(String, serde_json::Value)]>| {
        let mut state = json!({});
        let mut gate = json!({"claude": {"projections": {"claude:a": [{"window": "5h", "used_pct": 10.0}]}}});
        sampling::update_controls(&mut state, &mut gate, &["claude"], fleet, 1000.0, 2, |_, _| Ok(None)).unwrap();
        state["accounts"]["claude:a"]["workers"].clone()
    };
    assert_eq!(workers(Some(&fleet)), json!(3));
    assert_eq!(workers(None), serde_json::Value::Null);
}
