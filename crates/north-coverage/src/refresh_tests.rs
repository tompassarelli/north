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
}
