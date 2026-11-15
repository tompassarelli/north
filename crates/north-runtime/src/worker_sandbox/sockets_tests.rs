use std::os::unix::net::UnixListener;

use super::super::broker::Laid;

#[test]
fn the_runtime_socket_view_is_built_from_the_kept_list_alone() {
    let scratch = tempfile::tempdir().unwrap();
    let runtime = scratch.path();
    for directory in ["cc-socks", "systemd", "north-session-hosts", "agent-capacity-v1/handover"] {
        std::fs::create_dir_all(runtime.join(directory)).unwrap();
    }
    let bind = |name: &str| UnixListener::bind(runtime.join(name)).unwrap();
    let mut listening: Vec<UnixListener> = ["wayland-1", "north-kept.sock", "cc-socks/1.sock", "daemon.sock", "systemd/private", "north-ssh-0a1b", "north-session-hosts/0a1b.sock"].map(bind).into();
    let tmux = runtime.join("tmux-0");
    let names = |laid: &Laid| laid.kept.iter().map(|(path, _)| path.strip_prefix(runtime).unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
    let door = runtime.join("agent-capacity-v1/handover/0a1b.sock");
    listening.push(UnixListener::bind(&door).unwrap());
    let laid = Laid::of(runtime, &tmux, &[]);
    assert!(laid.hidden.contains(&door), "a queued run's hand-over door under the kept admission state is reachable: {:?}", laid.hidden);
    assert_eq!(names(&laid), [north_base::platform::admission_root::DIRECTORY, "north-kept.sock", "wayland-1"], "the runtime view holds an entry outside the kept list");
    for remade in ["daemon.sock", "systemd/private", "north-session-hosts/0a1b.sock"] {
        std::fs::remove_file(runtime.join(remade)).unwrap();
        listening.push(bind(remade));
    }
    listening.extend(["late.sock", "north-session-hosts/late.sock", "north-ssh-late"].map(bind));
    let now = Laid::of(runtime, &tmux, &[]);
    assert_eq!(names(&now), names(&laid), "the runtime view followed sockets made or remade after it was laid");
    assert!(laid.serves(&now), "a socket outside the kept list made or remade after the view was laid needed new namespaces, so a command already running in the old ones reaches it");
}
