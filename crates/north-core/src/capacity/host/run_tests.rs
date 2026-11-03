use std::time::Duration;

use super::json::Obj;
use super::request::Request;
use super::run::{cache_server_socket, scope_command, watch, watched_state};

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

fn age_queue(root: &std::path::Path) {
    std::fs::File::open(root.join("queue")).expect("queue directory").set_modified(std::time::UNIX_EPOCH).expect("age the queue");
}

#[test]
fn a_new_lease_or_a_new_ticket_changes_what_a_waiter_watches_but_a_rewritten_usage_sample_does_not() {
    let root = state_root();
    age_queue(root.path());
    let head = watch(Some("DEFER_RUN_QUEUE"));
    let behind = watch(Some("DEFER_QUEUED"));
    let (head_before, behind_before) = (watched_state(root.path(), head), watched_state(root.path(), behind));

    let leases = root.path().join("leases");
    for sample in 0..3 {
        std::fs::write(leases.join("aaaa.usage"), format!("{sample}\n")).expect("usage sample");
    }
    assert_eq!(watched_state(root.path(), head), head_before, "a usage sample woke a waiting head");
    assert_eq!(watched_state(root.path(), behind), behind_before, "a usage sample woke a waiting ticket");

    std::fs::write(leases.join("bbbb.json"), "{}\n").expect("lease");
    assert_ne!(watched_state(root.path(), head), head_before, "a new lease did not wake a waiting head");
    let (head_leased, behind_leased) = (watched_state(root.path(), head), watched_state(root.path(), behind));
    assert_ne!(behind_leased, behind_before, "a new lease did not wake a waiting ticket");

    age_queue(root.path());
    let behind_aged = watched_state(root.path(), behind);
    std::fs::write(root.path().join("queue").join("000000000000001-a.json"), "{}\n").expect("ticket");
    assert_eq!(watched_state(root.path(), head), head_leased, "a head watches no queue");
    assert_ne!(watched_state(root.path(), behind), behind_aged, "a new ticket did not wake a ticket behind");
}
