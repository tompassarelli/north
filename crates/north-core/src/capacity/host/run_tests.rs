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

fn state_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("scratch");
    for directory in ["leases", "queue"] {
        std::fs::create_dir(root.path().join(directory)).expect("state directory");
    }
    root
}

#[tokio::test]
async fn a_rewritten_usage_sample_does_not_wake_a_waiter_but_a_new_lease_does() {
    let root = state_root();
    let leases = root.path().join("leases");
    let usage = leases.join("aaaa.usage");
    let started = Instant::now();
    let writer = tokio::spawn(async move {
        for sample in 0..12 {
            std::fs::write(&usage, format!("{sample}\n")).expect("usage sample");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    state_change(root.path(), watch(Some("DEFER_RUN_QUEUE"))).await;
    writer.await.expect("writer");
    assert!(started.elapsed() >= Duration::from_millis(950), "a usage sample woke the waiter after {:?}", started.elapsed());

    let started = Instant::now();
    let lease = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        std::fs::write(leases.join("bbbb.json"), "{}\n").expect("lease");
    });
    state_change(root.path(), watch(Some("DEFER_RUN_QUEUE"))).await;
    lease.await.expect("lease writer");
    assert!(started.elapsed() < Duration::from_millis(900), "a new lease woke the waiter only after {:?}", started.elapsed());
}

#[tokio::test]
async fn a_ticket_behind_decides_again_as_soon_as_the_queue_changes() {
    let root = state_root();
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
