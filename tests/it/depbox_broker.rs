use std::os::unix::net::UnixListener;
use std::process::Command;

fn depbox(scratch: &std::path::Path, broker: &std::path::Path, target: &std::path::Path) -> std::process::Output {
    let home = scratch.join("home");
    let checkout = home.join("code/fixture/main");
    std::fs::create_dir_all(&checkout).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(&checkout).status().unwrap().success());
    Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(["depbox", "run", "--", "nc", "-U", "-w", "2"])
        .arg(target)
        .current_dir(&checkout)
        .env("HOME", &home)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("NORTH_BROKER_SOCKET", broker)
        .env("DEPBOX_ADMITTED", "1")
        .env_remove("DEPBOX")
        .env_remove("NORTH_WORKER_SANDBOXED")
        .output()
        .unwrap()
}

#[test]
fn dependency_code_in_depbox_cannot_connect_to_the_broker_socket() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("home/code/fixture/main/run");
    std::fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("broker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let outside = Command::new("nc").args(["-U", "-w", "2"]).arg(&socket).output().unwrap();
    assert!(listener.accept().is_ok(), "the probe cannot reach the listener even outside depbox: {}", String::from_utf8_lossy(&outside.stderr));
    let other = scratch.path().join("home/code/fixture/main/other");
    std::fs::create_dir_all(&other).unwrap();
    let control = UnixListener::bind(other.join("plain.sock")).unwrap();
    control.set_nonblocking(true).unwrap();
    let reached = depbox(scratch.path(), &socket, &other.join("plain.sock"));
    assert!(reached.status.success() && control.accept().is_ok(), "depbox itself did not run the probe: {}", String::from_utf8_lossy(&reached.stderr));
    let inside = depbox(scratch.path(), &socket, &socket);
    assert!(!inside.status.success(), "the broker socket answered inside depbox: {}", String::from_utf8_lossy(&inside.stderr));
    assert!(listener.accept().is_err(), "dependency code in depbox connected to the broker socket");
    let checkout = scratch.path().join("home/code/fixture/main");
    let victim = checkout.join("kept.txt");
    std::fs::write(&victim, "kept").unwrap();
    let handed_off = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(["depbox", "run", "--", "rm", "-f", "kept.txt"])
        .current_dir(&checkout)
        .env("HOME", scratch.path().join("home"))
        .env("DEPBOX_ADMITTED", "1")
        .env("NORTH_WORKER_REMOVAL_ROOTS", "[]")
        .env_remove("DEPBOX")
        .env_remove("NORTH_WORKER_SANDBOXED")
        .output()
        .unwrap();
    assert!(!handed_off.status.success() && victim.exists(), "depbox removed a path outside the roots the broker handed it: {}", String::from_utf8_lossy(&handed_off.stderr));
    let store = scratch.path().join("home/.local/share/makgora-build-inputs/store/base");
    let (pinned, unpinned, missing) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
    for entry in [&pinned, &unpinned] {
        std::fs::create_dir_all(store.join(entry)).unwrap();
    }
    let pin = |hash: &str| {
        std::fs::create_dir_all(checkout.join("pins")).unwrap();
        std::fs::write(checkout.join("pins/base"), hash).unwrap();
        std::fs::write(checkout.join(".safe-push"), "[land]\n\thost-input = ~/.local/share/makgora-build-inputs/store@pins :: bun test\n").unwrap();
        for step in [&["add", "pins", ".safe-push"][..], &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-qm", "pin"]] {
            assert!(Command::new("git").args(step).current_dir(&checkout).status().unwrap().success());
        }
    };
    let viewed = |entry: &str, view: &str| {
        Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
            .args(["depbox", "run", "--", "ls"])
            .arg(store.join(entry))
            .current_dir(&checkout)
            .env("HOME", scratch.path().join("home"))
            .env("DEPBOX_ADMITTED", "1")
            .env("SAFE_PUSH_LAND_STORE_VIEW", view)
            .env("SAFE_PUSH_LAND_HOST_INPUTS", store.join(&unpinned))
            .env_remove("DEPBOX")
            .env_remove("NORTH_WORKER_SANDBOXED")
            .output()
            .unwrap()
    };
    pin(&pinned);
    let seen = viewed(&pinned, "pinned");
    assert!(seen.status.success(), "the store entry the checkout's head pins is not in the sandbox: {}", String::from_utf8_lossy(&seen.stderr));
    let unseen = viewed(&unpinned, "pinned");
    assert!(!unseen.status.success(), "a store entry the head does not pin reached the sandbox through the environment");
    let elsewhere = viewed(&pinned, &"0".repeat(40));
    assert!(!elsewhere.status.success(), "a store view checked at another commit bound the entries the checkout's head pins now");
    pin(&missing);
    let refused = viewed(&pinned, "pinned");
    assert!(!refused.status.success() && String::from_utf8_lossy(&refused.stderr).contains(&missing), "a pinned store entry the host lacks did not refuse the run: {}", String::from_utf8_lossy(&refused.stderr));
}
