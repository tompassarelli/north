use super::*;

fn night_of_2026_11_08(now: f64) -> Facts {
    Facts {
        now,
        depth: 9,
        oldest_queued_at: Some(1_794_077_425.0),
        last_landed_at: Some(1_794_078_192.0),
        check_started_at: Some(1_794_078_195.0),
        check_p90_s: Some(204.0),
        main_at: Some(1_794_077_917.0),
        last_line: Some("safe-push: candidate batch of 8 ended without a verdict".into()),
    }
}

#[test]
fn the_lander_stall_of_2026_11_08_alarms_on_the_first_flow_tick_past_ten_minutes_of_unmoved_main() {
    assert!(!judge(&night_of_2026_11_08(1_794_078_300.0)).stalled, "19:05 UTC, main unmoved 6.4 min");
    let reading = judge(&night_of_2026_11_08(1_794_078_600.0));
    assert!(reading.stalled, "19:10 UTC (2:10 AM +07), main unmoved 11.4 min, over the 10 min floor: {reading:?}");
    assert_eq!(reading.stall_limit_minutes, 10.0);
}
