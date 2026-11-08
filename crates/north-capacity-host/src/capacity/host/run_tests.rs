use super::json::Obj;
use super::request::Request;
use super::run::{cache_server_socket, scope_command};

fn server_socket(root: &std::path::Path, class: &str) -> Option<std::ffi::OsString> {
    let request = Request { name: class.to_owned(), ..Request::default() };
    let admitted = Obj::new().put("lease", "00000000-0000-4000-8000-000000000001").put("scope", serde_json::json!({ "cpus": 6, "memoryMiB": 8192 }));
    let command = scope_command(root, &request, &admitted, Some(60), &["cargo".to_owned(), "build".to_owned()]);
    let cap = command.as_std().get_args().any(|argument| argument == "--property=TasksMax=4096");
    assert_eq!(cap, class != "native", "{class} task cap");
    command.as_std().get_envs().find(|(name, _)| *name == "SCCACHE_SERVER_UDS").and_then(|(_, value)| value.map(std::ffi::OsStr::to_os_string))
}

// KEEP(bug): each batch lease gets its own sccache server inside its scope; regression of 96854b985 (shared server ran in another session scope)
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

// KEEP(bug): the sccache socket path fits the 107-byte unix socket limit under a private desktop; regression of 09691aecf (SUN_LEN refusal)
#[test]
fn the_cache_server_socket_fits_a_unix_socket_path_under_the_private_desktops_runtime_directory() {
    const SUN_PATH_BYTES: usize = 107;
    let runtime = std::path::Path::new("/run/user/1000/private-desktop.0rJtMv/runtime");
    let root = runtime.join("agent-capacity-v1");
    let socket = cache_server_socket(&root, &super::machine::uuid());
    assert!(socket.as_os_str().len() <= SUN_PATH_BYTES, "{} is {} bytes", socket.display(), socket.as_os_str().len());
}

// KEEP(bug): firn-delivery-liveness lists no systemd on its PATH, so the lease its nix build waits for could never set its slice limits and the unit failed three times on 2026-11-06; north#671
#[test]
fn a_unit_whose_path_lacks_systemctl_gets_the_system_profiles() {
    let system = tempfile::tempdir().unwrap();
    let own = tempfile::tempdir().unwrap();
    for directory in [system.path(), own.path()] {
        std::fs::write(directory.join("systemctl"), "").unwrap();
    }
    let onto = |path: &std::path::Path| path.to_string_lossy().into_owned();
    assert_eq!(super::systemd::program_in("systemctl", Some("/nonexistent:/also-not"), system.path()), system.path().join("systemctl"));
    assert_eq!(super::systemd::program_in("systemctl", None, system.path()), system.path().join("systemctl"));
    assert_eq!(super::systemd::program_in("systemctl", Some(&onto(own.path())), system.path()), std::path::PathBuf::from("systemctl"));
    assert_eq!(super::systemd::program_in("systemd-run", Some("/nonexistent"), system.path()), std::path::PathBuf::from("systemd-run"));
}

fn scope_properties(class: &str, landing: bool) -> Vec<String> {
    let request = Request { name: class.to_owned(), landing, ..Request::default() };
    let admitted = Obj::new().put("lease", "00000000-0000-4000-8000-000000000002").put("scope", serde_json::json!({ "cpus": 6, "memoryMiB": 8192 }));
    let command = scope_command(std::path::Path::new("/nonexistent"), &request, &admitted, Some(60), &["cargo".to_owned(), "build".to_owned()]);
    command.as_std().get_args().map(|argument| argument.to_string_lossy().into_owned()).filter(|argument| argument.starts_with("--property=CPU")).collect()
}

// KEEP(bug): a heavy lease scope ran 10 cores at CPUQuota=infinity on a 20-core host at load 45; every lease scope is held to its charge and a land check outweighs the worker scopes; north#784
#[test]
fn every_lease_scope_carries_a_cpu_quota_equal_to_its_charge_and_a_land_check_outweighs_workers() {
    for class in ["heavy", "moderate", "exclusive", "tom"] {
        assert_eq!(scope_properties(class, false), ["--property=CPUQuota=600%"], "{class}");
    }
    assert_eq!(scope_properties("heavy", true), ["--property=CPUQuota=600%", "--property=CPUWeight=1000"]);
}

#[test]
fn cargo_in_a_lease_runs_as_many_jobs_as_the_lease_holds_cpus_not_its_scopes_ceiling() {
    let request = Request { name: "heavy".to_owned(), landing: true, ..Request::default() };
    let admitted = Obj::new().put("lease", "00000000-0000-4000-8000-000000000003").put("requestedCpus", 4).put("scope", serde_json::json!({ "cpus": 20, "memoryMiB": 8192 }));
    let command = scope_command(std::path::Path::new("/nonexistent"), &request, &admitted, Some(60), &["cargo".to_owned(), "build".to_owned()]);
    let jobs = command.as_std().get_envs().find(|(name, _)| *name == "CARGO_BUILD_JOBS").and_then(|(_, value)| value.map(std::ffi::OsStr::to_os_string));
    assert_eq!(jobs.as_deref(), Some(std::ffi::OsStr::new("4")), "on 2026-11-08 at 8:02 AM a 4-CPU land-check lease on the laptop ran cargo with CARGO_BUILD_JOBS=20, its scope's ceiling, and held 41 rustc processes at load 86");
}

fn launched(root: &std::path::Path, launcher: &str) -> (crate::capacity::subject_start::Exit, Option<serde_json::Value>) {
    let lease = "00000000-0000-4000-8000-000000000004";
    let request = Request { name: "heavy".to_owned(), landing: true, ..Request::default() };
    let admitted = Obj::new().put("lease", lease).put("scope", serde_json::json!({ "cpus": 2, "memoryMiB": 1024 }));
    let command = ["bash".to_owned(), "bin/catalog-owner-check".to_owned()];
    let scoped = scope_command(root, &request, &admitted, Some(60), &command);
    let witness = crate::capacity::subject_start::witness(root, lease);
    let named = scoped.as_std().get_envs().find(|(name, _)| *name == crate::capacity::subject_start::ENV).and_then(|(_, value)| value.map(std::path::PathBuf::from));
    assert_eq!(named.as_deref(), Some(witness.as_path()), "the scope names the lease's start witness to the wrappers inside it");
    let program = root.join("systemd-run");
    north_base::foundation::shim::write_script(&program, &format!("{launcher}\n"));
    crate::capacity::subject_start::arm(&witness);
    let status = std::process::Command::new(&program).env(crate::capacity::subject_start::ENV, &witness).stderr(std::process::Stdio::null()).status().unwrap();
    let exit = crate::capacity::subject_start::Exit::judge(status.code().unwrap(), &witness);
    (exit, super::run::command_exit(&request, "land-check", &command, &admitted, exit).map(|line| line.value()))
}

// KEEP(bug): south 6dd9582d2d was reverted as 47778f0 on 2026-11-08 because systemd-run's own "Failed to start transient scope unit: Remote peer disconnected" exit 1 was reported as bin/catalog-owner-check's exit; north#892
#[test]
fn a_lease_whose_scope_never_started_its_subject_exits_no_verdict_not_the_launchers_failure() {
    let root = tempfile::tempdir().unwrap();
    let (exit, line) = launched(root.path(), "echo 'Failed to start transient scope unit: Remote peer disconnected' >&2; exit 1");
    assert_eq!(exit.code(), north_base::foundation::exit::NO_VERDICT_STATUS);
    let line = line.expect("a decision line");
    assert_eq!((line["decision"].as_str(), line["launcherExitCode"].as_i64()), (Some("SUBJECT_NOT_STARTED"), Some(1)));
    let (exit, line) = launched(root.path(), "touch \"$NORTH_SUBJECT_STARTED\"; exit 1");
    assert_eq!(exit.code(), 1, "a subject that started and failed keeps its own exit");
    assert_eq!(line.expect("a decision line")["decision"].as_str(), Some("COMMAND_EXITED"));
}
