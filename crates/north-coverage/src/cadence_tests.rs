use super::*;

#[test]
fn a_missing_or_day_old_map_is_due_for_a_refresh_and_a_fresh_one_is_not() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("north.json");
    assert_eq!(due(&path, SystemTime::UNIX_EPOCH).as_deref(), Some("no map yet"));
    std::fs::write(&path, "{}").unwrap();
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(due(&path, written + Duration::from_secs(3600)), None);
    assert_eq!(due(&path, written + STALE_AFTER + Duration::from_secs(3600)).as_deref(), Some("the map is 25 h old"));
    assert_eq!(unknown_reason(0, 0), None, "a build that lists no test starts no refresh");
    assert_eq!(unknown_reason(1, 10), None, "exactly the share does not pass it");
    let reason = unknown_reason(2, 10).unwrap();
    assert!(reason.contains("2 of the 10") && reason.contains("over 10%"), "{reason}");
}

#[test]
fn a_repository_with_no_checkout_to_refresh_in_starts_no_refresh_unit() {
    let served = tempfile::tempdir().unwrap();
    let why = start(served.path(), "no map yet").unwrap_err();
    assert!(why.contains("has no checkout at") && why.ends_with("so none starts (no map yet)"), "{why}");
}
