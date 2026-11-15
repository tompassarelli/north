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

const PROBE: &str = "NORTH_1397_SOCKET_PROBE";

#[test]
fn a_confined_command_cannot_connect_to_a_hidden_socket_of_each_kind() {
    use super::super::{Context, Reader, confine, executable, readable, removable, sockets};
    if let Some(targets) = std::env::var_os(PROBE) {
        use std::os::unix::fs::MetadataExt;
        let context = Context::from_env("probe");
        let runtime = sockets::runtime(&context);
        let hidden = sockets::hidden(&runtime, &sockets::multiplexer(context.uid, std::env::var_os("TMUX_TMPDIR")), &sockets::ssh_reach(&context.home, context.uid, context.agent_socket.as_deref()));
        sockets::overlay(&runtime, &hidden).unwrap();
        let roots = removable(&context);
        assert!(matches!(confine(Some(&roots), &readable(&context, Reader::of(true)), Some(&executable(&context)), None, &[]), Ok(landlock::RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
        for target in std::env::split_paths(&targets) {
            match std::os::unix::net::UnixStream::connect(&target) {
                Ok(_) => println!("reached {}", target.display()),
                Err(error) => println!("connect {} refused: {error:?}", target.display()),
            }
        }
        for entry in std::fs::read_dir(&runtime).into_iter().flatten().flatten() {
            let path = entry.path();
            println!("view {} {:?}", path.display(), std::fs::symlink_metadata(&path).map(|metadata| (metadata.file_type(), metadata.mode() & 0o7777)));
        }
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let (home, runtime, tmpdir) = (scratch.path().join("home"), scratch.path().join("run"), scratch.path().join("tmp"));
    let uid = rustix::process::getuid().as_raw();
    let agent = scratch.path().join("agent");
    let hidden_kinds = [runtime.join("bus"), runtime.join("north-session-hosts/0a1b.sock"), runtime.join("north-broker/broker.sock"), runtime.join(format!("{}/handover/0a1b.sock", north_base::platform::admission_root::DIRECTORY)), tmpdir.join(format!("tmux-{uid}/default")), agent.clone()];
    let kept = [runtime.join("wayland-1"), runtime.join("north-gh-broker/broker.sock"), runtime.join("north-outside/broker.sock")];
    std::fs::create_dir_all(&home).unwrap();
    let _listening: Vec<UnixListener> = hidden_kinds.iter().chain(&kept).map(|path| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        UnixListener::bind(path).unwrap()
    }).collect();
    let output = std::process::Command::new("unshare")
        .args(["--user", "--map-current-user", "--keep-caps", "--mount", "--"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::sockets::a_confined_command_cannot_connect_to_a_hidden_socket_of_each_kind", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("TMUX_TMPDIR", &tmpdir)
        .env("SSH_AUTH_SOCK", &agent)
        .env(PROBE, std::env::join_paths(hidden_kinds.iter().chain(&kept)).unwrap())
        .current_dir(&home)
        .output()
        .unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && stdout.contains("running 1 test"), "the confined probe did not run: {stdout}{stderr}");
    let reached: Vec<&str> = stdout.lines().filter_map(|line| line.strip_prefix("reached ")).collect();
    let expected: Vec<String> = kept.iter().map(|path| path.display().to_string()).collect();
    assert_eq!(reached, expected, "a confined command reached a hidden socket, or missed a kept one: {stderr}\n{stdout}");
}
