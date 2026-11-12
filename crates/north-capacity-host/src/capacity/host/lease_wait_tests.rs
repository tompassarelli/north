use super::json::Obj;
use super::lease_waiting_under;
use super::state::enqueue;

#[test]
fn a_lease_in_the_capacity_queue_is_waiting_and_an_empty_queue_is_not() {
    let root = tempfile::tempdir().unwrap();
    assert!(!lease_waiting_under(root.path(), None));
    enqueue(root.path(), Obj::new().put("class", "heavy"));
    assert!(lease_waiting_under(root.path(), None));
}

#[test]
fn a_warm_job_does_not_count_its_own_queued_run_as_a_waiting_lease() {
    let root = tempfile::tempdir().unwrap();
    enqueue(root.path(), Obj::new().put("class", "heavy"));
    assert!(!lease_waiting_under(root.path(), Some(u64::from(std::process::id()))));
}
