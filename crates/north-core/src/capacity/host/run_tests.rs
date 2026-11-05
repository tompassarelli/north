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
fn the_cache_server_socket_fits_a_unix_socket_path_under_the_private_desktops_runtime_directory() {
    const SUN_PATH_BYTES: usize = 107;
    let runtime = std::path::Path::new("/run/user/1000/private-desktop.0rJtMv/runtime");
    let root = runtime.join("agent-capacity-v1");
    let socket = cache_server_socket(&root, &super::machine::uuid());
    assert!(socket.as_os_str().len() <= SUN_PATH_BYTES, "{} is {} bytes", socket.display(), socket.as_os_str().len());
}

fn quota(class: &str, landing: bool, cpus: f64) -> Vec<String> {
    let root = tempfile::tempdir().expect("scratch");
    let request = Request { name: class.to_owned(), landing, ..Request::default() };
    let admitted = Obj::new().put("lease", "00000000-0000-4000-8000-000000000001").put("scope", serde_json::json!({ "cpus": cpus, "memoryMiB": 8192 }));
    let command = scope_command(root.path(), &request, &admitted, Some(60), &["true".to_owned()]);
    command.as_std().get_args().map(|argument| argument.to_string_lossy().into_owned()).filter(|argument| argument.starts_with("--property=CPU") || argument.starts_with("--slice=")).collect()
}

#[test]
fn a_scope_never_gets_a_zero_cpu_quota_and_batch_scopes_get_none() {
    assert_eq!(quota("runtime", false, 0.0), ["--slice=agent-capacity.slice"], "a zero-CPU grant is weight-only, never CPUQuota=0%");
    assert_eq!(quota("runtime", false, 2.0), ["--slice=agent-capacity.slice", "--property=CPUQuota=200%"]);
    assert_eq!(quota("heavy", false, 6.0), ["--slice=agent-batch.slice"]);
    assert_eq!(quota("moderate", false, 2.0), ["--slice=agent-batch.slice"]);
    assert_eq!(quota("heavy", true, 6.0), ["--slice=agent-capacity.slice", "--property=CPUQuota=600%"], "a landing check keeps normal weight");
}

#[test]
fn a_host_whose_reserve_covers_its_cores_still_grants_one_cpu() {
    for profile in ["attended", "unattended"] {
        assert_eq!(super::policy::aggregate_cpus(profile, 1.0, None), 1.0, "{profile}");
        let request = Request { name: "runtime".to_owned(), resources: Some(super::policy::Resources { cpus: 2.0, memory_mib: 8192.0 }), ..Request::default() };
        let granted = super::request::grant(profile, 1.0, None, 65536.0, &request);
        assert_eq!(granted.charged.cpus, 1.0, "{profile}: a clamp to the 1-CPU host is 1, never 0");
    }
    assert_eq!(super::policy::aggregate_cpus("attended", 24.0, None), 20.0);
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

#[test]
fn a_tom_class_command_that_fails_under_the_gaming_hold_prints_its_exit_not_a_bare_reserved_released_pair() {
    let request = Request { name: "tom".to_owned(), ..Request::default() };
    let admitted = Obj::new().put("decision", "RESERVED").put("reason", "RUN").put("class", "tom").put("lease", "00000000-0000-4000-8000-000000000001");
    let command = ["touch".to_owned(), "target/1hz-logs/touched".to_owned()];
    let line = super::run::command_exit(&request, "wisp-1hz-opus", &command, &admitted, 1).expect("a nonzero exit is reported").value();
    assert_eq!(line["decision"], "COMMAND_EXITED");
    assert_eq!(line["exitCode"], 1);
    assert_eq!(line["program"], "touch");
    assert_eq!(line["class"], "tom");
    assert_eq!(line["admission"], "RUN", "the gaming hold admitted the class; the command itself failed");
    assert_eq!(line["lease"], "00000000-0000-4000-8000-000000000001");
    assert!(super::run::command_exit(&request, "wisp-1hz-opus", &command, &admitted, 0).is_none());
}

#[test]
fn state_resolved_from_a_private_desktops_runtime_directory_is_the_hosts_state() {
    use super::state::state_root;
    let host = std::path::PathBuf::from("/run/user/1000/agent-capacity-v1");
    assert_eq!(state_root("/run/user/1000", 1000), host);
    assert_eq!(state_root("/run/user/1000/private-desktop.TFxebN/runtime", 1000), host);
    assert_eq!(state_root("/run/user/1000/private-desktop.0rJtMv", 1000), host);
    assert_eq!(state_root("/run/user/1001/private-desktop.TFxebN/runtime", 1000), std::path::PathBuf::from("/run/user/1001/private-desktop.TFxebN/runtime/agent-capacity-v1"));
    assert_eq!(state_root("/tmp/scratch", 1000), std::path::PathBuf::from("/tmp/scratch/agent-capacity-v1"));
}
