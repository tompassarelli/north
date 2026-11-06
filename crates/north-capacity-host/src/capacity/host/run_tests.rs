use super::json::Obj;
use super::request::Request;
use super::run::{cache_server_socket, scope_command};

fn server_socket(root: &std::path::Path, class: &str) -> Option<std::ffi::OsString> {
    let request = Request { name: class.to_owned(), ..Request::default() };
    let admitted = Obj::new().put("lease", "00000000-0000-4000-8000-000000000001").put("scope", serde_json::json!({ "cpus": 6, "memoryMiB": 8192 }));
    let command = scope_command(root, &request, &admitted, Some(60), &["cargo".to_owned(), "build".to_owned()]);
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
