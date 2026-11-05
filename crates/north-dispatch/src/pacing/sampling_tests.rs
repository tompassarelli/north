use super::sampling;
use serde_json::json;

#[test]
fn a_failing_read_keeps_the_last_good_sample_time_and_retries_within_ten_minutes() {
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
}
