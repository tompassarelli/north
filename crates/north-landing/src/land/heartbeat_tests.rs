use super::*;

#[test]
fn a_step_held_past_the_threshold_is_named_once_then_again_after_the_repeat_gap() {
    assert!(!stuck(Step::Push, STUCK - Duration::from_secs(1), None));
    assert!(stuck(Step::Push, STUCK, None));
    assert!(!stuck(Step::Push, STUCK + Duration::from_secs(10), Some(STUCK)));
    assert!(stuck(Step::Push, STUCK + REPEAT, Some(STUCK)));
    assert!(!stuck(Step::Between, STUCK * 10, None));
}

#[test]
fn the_heartbeat_reports_the_step_last_entered() {
    let heartbeat = Heartbeat::default();
    heartbeat.enter(Step::Prepare);
    assert_eq!(heartbeat.current().0, Step::Prepare);
    heartbeat.enter(Step::Speculate);
    assert_eq!(heartbeat.current().0, Step::Speculate);
}
