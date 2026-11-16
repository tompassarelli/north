use super::create;
use crate::lanes::reap_tests::Container;

#[test]
fn a_lane_name_review_fetch_would_refuse_is_refused_at_creation() {
    let container = Container::new();
    let lane = container.lane("CapLane-1");
    let refused = create(&lane, "north#1650", None).unwrap_err();
    assert!(refused.contains("is not a lane name"), "{refused}");
    assert!(!lane.exists());
}
