
use serde_json::json;

use super::{curl_config, rows_from};

const HEADERS: &str = "HTTP/2 200 \r\ncontent-type: application/json\r\nAnthropic-Ratelimit-Unified-5h-Status: allowed\r\nanthropic-ratelimit-unified-5h-utilization: 0.37\r\nanthropic-ratelimit-unified-5h-reset: 1793118000\r\nanthropic-ratelimit-unified-7d-utilization: 0.0449\r\nanthropic-ratelimit-unified-7d-reset: 1793692800\r\nanthropic-ratelimit-unified-representative-claim: five_hour\r\n\r\n";

#[test]
fn a_messages_response_reports_the_session_and_weekly_windows_as_usage_rows() {
    let rows = rows_from("d", HEADERS, 1793100000.0).unwrap();
    let window = |name: &str| rows.iter().find(|row| row["name"] == name).unwrap().clone();
    assert_eq!(rows.len(), 2);
    assert_eq!((window("session")["used_pct"].clone(), window("session")["window_min"].clone(), window("session")["resets_at"].clone()), (json!(37.0), json!(300), json!("2026-10-27T16:20:00Z")));
    assert_eq!((window("weekly_all")["used_pct"].clone(), window("weekly_all")["resets_at"].clone()), (json!(4.0), json!("2026-11-03T08:00:00Z")));
    assert!(rows.iter().all(|row| row["provider"] == "claude" && row["account"] == "d" && row["kind"] == "window"));
}

#[test]
fn a_token_that_could_add_curl_config_lines_is_refused_without_echoing_it() {
    let config = curl_config("setup-token").unwrap();
    assert_eq!(config.lines().count(), 4);
    for hostile in ["", "two words", "line\nheader = \"x: y\"", "quote\"", "back\\slash"] {
        let refusal = curl_config(hostile).unwrap_err();
        assert!(!refusal.contains("quote") && !refusal.contains("slash"), "{refusal}");
    }
}
