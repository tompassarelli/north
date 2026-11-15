use super::*;

#[test]
fn the_lander_stall_of_2026_11_15_is_named_with_eleven_lanes_waiting_on_a_landing_26_minutes_old() {
    let now = 1_794_107_820.0;
    let facts = Facts {
        now,
        depth: 11,
        oldest_queued_at: Some(now - 34.0 * 60.0),
        last_landed_at: Some(now - 26.0 * 60.0),
        check_started_at: None,
        check_p90_s: Some(900.0),
        last_line: None,
    };
    let reading = judge(&facts);
    assert!(reading.stalled, "{reading:?}");
    assert_eq!(reading.stall_limit_minutes, 5.0);
    assert_eq!(reading.reasons, vec!["11 queued with no landing for 26m > 5m".to_owned()]);
    let cold: Vec<Vec<serde_json::Value>> = [1, 1, 1, 6].iter().map(|minutes| vec![serde_json::json!({ "name": "test", "conclusion": "success", "started_at": "2026-11-11T15:00:00Z", "completed_at": format!("2026-11-11T15:0{minutes}:00Z"), "steps": [{ "conclusion": "success" }] })]).collect();
    assert_eq!(super::super::stall::job_slowest(&cold).get("test"), Some(&6.0), "a cold-cache run in the history sets the job's limit, so the next one is not a stall");
}
