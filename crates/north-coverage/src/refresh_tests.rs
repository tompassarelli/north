use super::*;

#[test]
fn a_claim_left_by_a_process_that_is_not_a_refresh_is_taken_over_and_released_on_drop() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("north.refreshing");
    std::fs::write(&path, std::process::id().to_string()).unwrap();
    let running = Running::claim(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), std::process::id().to_string());
    drop(running);
    assert!(!path.exists());
    let script = crate::hosts::script("north");
    assert!(script.contains("/north/coverage") && script.contains("north.json") && script.contains("graph ingest coverage"), "a build host is sent the map and ingests it: {script}");
}
