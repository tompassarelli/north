use serde_json::{Value, json};

use super::{header_rows, stream_rows};

const AT: f64 = 1_792_000_000.0;

fn event(info: Value) -> Value {
    json!({"type": "rate_limit_event", "session_id": "s", "rate_limit_info": info})
}

fn pick<'a>(rows: &'a [Value], kind: &str, name: &str) -> &'a Value {
    rows.iter().find(|row| row["kind"] == kind && row["name"] == name).unwrap_or_else(|| panic!("no {kind} row {name} in {rows:?}"))
}

#[test]
fn a_rate_limit_event_writes_a_stream_row_for_every_unified_window_scoped_included() {
    let rows = stream_rows(
        &event(json!({"status": "allowed", "resetsAt": 1792049400, "rateLimitType": "five_hour", "unifiedWindows": {
            "five_hour": {"utilization": 0.73, "resetsAt": 1792049400},
            "seven_day": {"utilization": 0.19, "resetsAt": 1792634400},
            "seven_day_opus": {"utilization": 0.04, "resetsAt": 1792670400},
            "seven_day_sonnet": {"utilization": 0.5}
        }})),
        AT,
    );
    let windows: Vec<_> = rows.iter().filter(|row| row["kind"] == "window").collect();
    assert_eq!(windows.len(), 4);
    assert!(windows.iter().all(|row| row["provider"] == "claude" && row["source"].as_str().unwrap().contains("stream")));
    let session = pick(&rows, "window", "session");
    assert_eq!((session["used_pct"].clone(), session["window_min"].clone(), session["resets_at"].clone()), (json!(73.0), json!(300), json!("2026-10-15T07:30:00Z")));
    assert_eq!(pick(&rows, "window", "weekly_all")["window_min"], 10080);
    assert_eq!(pick(&rows, "window", "weekly_scoped/Opus")["used_pct"], 4.0);
    assert!(pick(&rows, "window", "weekly_scoped/Sonnet")["resets_at"].is_null());
    assert!(pick(&rows, "status", "session")["source"].as_str().unwrap().ends_with("status=allowed"));
    assert!(rows.iter().all(|row| row["kind"] != "limit"));
}

#[test]
fn a_rejected_status_writes_a_limit_row_with_the_reset_the_event_names() {
    let rejected = event(json!({"status": "rejected", "resetsAt": 1792084800, "rateLimitType": "five_hour"}));
    let rows = stream_rows(&rejected, AT);
    let limit = pick(&rows, "limit", "session");
    assert_eq!((limit["resets_at"].clone(), limit["used_pct"].clone()), (json!("2026-10-15T17:20:00Z"), json!(100.0)));
    assert!(limit["source"].as_str().unwrap().contains("stream"));
    assert!(pick(&rows, "status", "session")["source"].as_str().unwrap().ends_with("status=rejected"));

    let named = event(json!({"status": "rejected", "rateLimitType": "seven_day", "unifiedWindows": {"seven_day": {"utilization": 1.0, "resetsAt": 1792670400}}}));
    assert_eq!(pick(&stream_rows(&named, AT), "limit", "weekly_all")["resets_at"], "2026-10-22T12:00:00Z");
}

#[test]
fn frames_that_are_not_rate_limit_events_write_nothing() {
    assert!(stream_rows(&json!({"type": "assistant"}), AT).is_empty());
    assert!(stream_rows(&event(json!({})), AT).is_empty());
}

const HEADERS: &str = "HTTP/2 200 \r\nAnthropic-Ratelimit-Unified-5h-Status: allowed_warning\r\nanthropic-ratelimit-unified-5h-utilization: 0.37\r\nanthropic-ratelimit-unified-5h-reset: 1793118000\r\nanthropic-ratelimit-unified-7d-status: allowed\r\nanthropic-ratelimit-unified-7d-utilization: 0.0449\r\nanthropic-ratelimit-unified-7d-reset: 1793692800\r\nanthropic-ratelimit-unified-representative-claim: five_hour\r\n\r\n";

fn limits(rows: Vec<Value>) -> Vec<(Value, Value)> {
    rows.into_iter().filter(|row| row["kind"] == "limit").map(|row| (row["name"].clone(), row["resets_at"].clone())).collect()
}

#[test]
fn a_messages_response_reports_each_window_with_its_status_as_rows_of_the_account() {
    let rows = header_rows("d", HEADERS, 1793100000.0).unwrap();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row["provider"] == "claude" && row["account"] == "d" && row["source"].as_str().unwrap().contains("headers")));
    let session = pick(&rows, "window", "session");
    assert_eq!((session["used_pct"].clone(), session["window_min"].clone(), session["resets_at"].clone()), (json!(37.0), json!(300), json!("2026-10-27T16:20:00Z")));
    assert_eq!((pick(&rows, "window", "weekly_all")["used_pct"].clone(), pick(&rows, "window", "weekly_all")["resets_at"].clone()), (json!(4.0), json!("2026-11-03T08:00:00Z")));
    assert!(pick(&rows, "status", "session")["source"].as_str().unwrap().ends_with("status=allowed_warning"));
    assert!(pick(&rows, "status", "weekly_all")["source"].as_str().unwrap().ends_with("status=allowed"));
    assert!(limits(rows).is_empty());
}

#[test]
fn a_response_without_unified_headers_is_an_error_not_an_empty_reading() {
    assert!(header_rows("d", "HTTP/2 401 \r\ncontent-type: application/json\r\n\r\n", 1.0).is_err());
}

#[test]
fn a_rejected_window_or_a_429_with_a_reset_records_a_limit_row() {
    let reset = (json!("session"), json!("2026-10-27T16:20:00Z"));
    let rejected = HEADERS.replace("5h-Status: allowed_warning", "5h-Status: rejected");
    assert_eq!(limits(header_rows("d", &rejected, 1.0).unwrap()), std::slice::from_ref(&reset));
    let refused = HEADERS.replace("HTTP/2 200", "HTTP/2 429");
    assert_eq!(limits(header_rows("d", &refused, 1.0).unwrap()), [reset]);
    let bare = refused.replace("anthropic-ratelimit-unified-5h-reset: 1793118000\r\n", "");
    assert!(limits(header_rows("d", &bare, 1.0).unwrap()).is_empty());
}
