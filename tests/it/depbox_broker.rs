use std::os::unix::net::UnixListener;
use std::process::Command;

fn depbox(scratch: &std::path::Path, broker: &std::path::Path, target: &std::path::Path) -> std::process::Output {
    let home = scratch.join("home");
    let checkout = home.join("code/fixture/main");
    std::fs::create_dir_all(&checkout).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(&checkout).status().unwrap().success());
    Command::new(env!("CARGO_BIN_EXE_north"))
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
}
