use std::time::{Duration, Instant};

use super::json::Obj;
use super::request::Request;
use super::run::{cache_server_socket, scope_command, state_change, watch};

fn server_socket(root: &std::path::Path, class: &str) -> Option<std::ffi::OsString> {
    let request = Request { name: class.to_owned(), ..Request::default() };
    let admitted = Obj::new().put("lease", "00000000-0000-4000-8000-000000000001").put("scope", serde_json::json!({ "cpus": 6, "memoryMiB": 8192 }));
    let command = scope_command(root, &request, &admitted, Some(60), &["cargo".to_owned(), "build".to_owned()]);
    command.as_std().get_envs().find(|(name, _)| *name == "SCCACHE_SERVER_UDS").and_then(|(_, value)| value.map(std::ffi::OsStr::to_os_string))
}

#[test]
fn each_batch_lease_compiles_through_its_own_cache_server_inside_its_scope() {
    let root = tempfile::tempdir().expect("scratch");
    for class in ["heavy", "moderate", "tom"] {
        let socket = server_socket(root.path(), class).unwrap_or_else(|| panic!("{class} lease shares the host's cache server"));
        assert_eq!(std::path::PathBuf::from(&socket), cache_server_socket(root.path(), "00000000-0000-4000-8000-000000000001"), "{class}");
    }
    assert!(root.path().join("sccache").is_dir());
    assert_eq!(server_socket(root.path(), "native"), None);
}

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
