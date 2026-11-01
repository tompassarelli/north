use std::time::{Duration, Instant};

use super::run::{state_change, watch};

#[test]
fn a_ticket_behind_an_earlier_one_watches_the_queue_and_polls_rarely_while_the_head_polls_each_second() {
    let behind = watch(Some("DEFER_QUEUED"));
    assert!(behind.queue);
    assert_eq!(behind.poll, Duration::from_secs(10));
    for reason in [Some("DEFER_RUN_QUEUE"), Some("DEFER_CPU_PRESSURE"), Some("DEFER_GPU_SLOTS"), None] {
        let head = watch(reason);
        assert!(!head.queue, "{reason:?}");
        assert_eq!(head.poll, Duration::from_secs(1), "{reason:?}");
    }
}

#[tokio::test]
async fn a_ticket_behind_decides_again_as_soon_as_the_queue_changes() {
    let root = tempfile::tempdir().expect("scratch");
    for directory in ["leases", "queue"] {
        std::fs::create_dir(root.path().join(directory)).expect("state directory");
    }
    let queue = root.path().join("queue");
    let started = Instant::now();
    let change = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        std::fs::write(queue.join("000000000000001-a.json"), "{}\n").expect("ticket");
    });
    state_change(root.path(), watch(Some("DEFER_QUEUED"))).await;
    change.await.expect("writer");
    assert!(started.elapsed() < Duration::from_secs(3), "woke after {:?}", started.elapsed());
}
