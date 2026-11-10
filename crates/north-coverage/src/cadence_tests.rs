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
}
