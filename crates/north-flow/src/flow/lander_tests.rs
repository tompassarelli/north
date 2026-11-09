use super::*;
use crate::foundation::clock::Timestamp;
use north_base::land_ticket::LandTicket;

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

#[test]
fn a_log_without_a_verdict_is_never_a_running_candidate_check() {
    let queue = tempfile::tempdir().unwrap();
    let logs = queue.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let now = 1_794_131_000.0;
    let began = format!("{}", ((now - 540.0) * 1e6) as u64);
    std::fs::write(logs.join(format!("{began}-sheriff-nix-shell.lane-gates.log")), "crate-lines: no crate over its cap grew\n").unwrap();
    std::fs::write(logs.join(format!("{began}-lane-check.log")), "cargo nextest run\n").unwrap();
    let facts = read(queue.path(), now);
    assert_eq!(facts.check_started_at, None, "{:?}", facts.check_started_at);
    assert!(judge(&facts).reasons.is_empty());
}

#[test]
fn a_ticket_in_phase_checking_is_the_running_check_from_its_transition_time() {
    let queue = tempfile::tempdir().unwrap();
    let now = 1_794_131_000.0;
    let began = now - 540.0;
    let ticket = queue.path().join("1794129000-sheriff-nix-shell.json");
    std::fs::write(&ticket, "{}").unwrap();
    land_ticket::queued(&ticket, "the test queued it").unwrap();
    let mut store = land_ticket::open(queue.path()).unwrap();
    store.transition::<LandTicket>(&land_ticket::instance(queue.path(), ticket.file_name().unwrap()), Phase::Checking, "a land check took it", Timestamp::from_unix_seconds_f64(began)).unwrap().unwrap();
    let facts = read(queue.path(), now);
    assert_eq!(facts.check_started_at, Some(began), "{:?}", facts.check_started_at);
}
