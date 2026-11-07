use super::problems;

const RACING: &str = r#"
#[test]
fn a_frozen_land_check_times_out_only_after_its_timeout_plus_the_time_it_was_frozen() {
    let mut running = start();
    NAP(Duration::from_secs(2));
    assert!(!running.output().contains(deadline), "ended by its wall-clock timeout");
    let begun = Instant::now();
    finish();
    assert!(begun.TOOK < Duration::from_secs(3), "slow");
}
"#;

const DRIVEN: &str = r#"
fn wait_until(limit: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while !condition() {
        if started.TOOK > limit {
            return false;
        }
        NAP(Duration::from_millis(20));
    }
    true
}

#[test]
fn a_frozen_land_check_waits_for_the_driven_clock() {
    clock.set(3_000);
    assert!(wait_until(Duration::from_secs(60), || started.exists()));
    // NAP(Duration::from_secs(2)) once stood here
    let _ = "sh -c 'touch x; sleep 40'";
}
"#;

fn planted(text: &str) -> String {
    text.replace("NAP", "std::thread::sleep").replace("TOOK", "elapsed()")
}

// KEEP(bug): on 2026-11-07 a frozen land check test slept 2 s and judged a 1 s wall deadline, failed on a loaded gex45 and ccx33 and paused every landing (north#783)
#[test]
fn a_test_that_sleeps_or_asserts_on_elapsed_time_outside_a_polling_loop_is_refused_where_a_clock_driven_poll_passes() {
    let found = problems("tests/it/safe_push/session.rs", &planted(RACING));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("session.rs:5 sleeps outside a polling loop"), "{found:?}");
    assert!(found[1].contains("session.rs:9 asserts on elapsed wall-clock time"), "{found:?}");
    assert_eq!(problems("tests/it/safe_push/session.rs", &planted(DRIVEN)), Vec::<String>::new());
}
