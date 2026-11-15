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
    assert!(north_capacity_host::capacity::depbox_broker::hides(std::path::Path::new(north_base::platform::broker_client::AGENT_SOCKET)), "depbox leaves the agent broker socket in reach");
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

#[test]
fn a_confined_check_cannot_plant_the_north_or_the_crate_source_a_later_unconfined_run_of_another_tree_uses() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let git = |directory: &std::path::Path, arguments: &[&str]| {
        let output = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(arguments).current_dir(directory).output().unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let other = home.join("code/other/main");
    std::fs::create_dir_all(other.join("bin")).unwrap();
    std::fs::copy(crate::support::manifest_dir().join("bin/head-north"), other.join("bin/head-north")).unwrap();
    git(&other, &["init", "-q"]);
    git(&other, &["add", "."]);
    git(&other, &["commit", "-qm", "head-north"]);
    let tree = git(&other, &["rev-parse", "HEAD^{tree}"]);
    let live = home.join(".local/state/north/current/north");
    std::fs::create_dir_all(live.parent().unwrap()).unwrap();
    std::fs::write(&live, "#!/bin/sh\nif [ \"$1 $2\" = 'runtime covers' ]; then printf %s \"$0\"; exit 0; fi\necho live north\n").unwrap();
    std::fs::set_permissions(&live, <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755)).unwrap();
    let source = home.join(".cargo/registry/src/index.crates.io-0/dependency-1.0.0/src/lib.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "pub fn kept() {}\n").unwrap();
    let lane = home.join("code/fixture/main");
    std::fs::create_dir_all(&lane).unwrap();
    git(&lane, &["init", "-q"]);
    let planted = home.join(format!(".cache/north/head-north/{tree}/north"));
    let plant = format!("mkdir -p {0} && printf '#!/bin/sh\\necho planted north\\n' >{0}/north && chmod +x {0}/north; printf 'pub fn planted() {{}}\\n' >{1}; true", planted.parent().unwrap().display(), source.display());
    let confined = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(["depbox", "run", "--", "sh", "-c", &plant])
        .current_dir(&lane)
        .env("HOME", &home)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("DEPBOX_ADMITTED", "1")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("DEPBOX")
        .env_remove("NORTH_WORKER_SANDBOXED")
        .output()
        .unwrap();
    assert!(confined.status.success() && planted.is_file(), "the confined check did not run in depbox: {}", String::from_utf8_lossy(&confined.stderr));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "pub fn kept() {}\n", "a build in depbox rewrote an extracted crate source a later build outside it compiles");
    let mut unconfined = Command::new("bash");
    unconfined.args(["bin/head-north", "--version"]).current_dir(&other).env("HOME", &home).env("XDG_STATE_HOME", home.join(".local/state"));
    for name in ["XDG_CACHE_HOME", "SAFE_PUSH_LAND_HEAD", "SAFE_PUSH_LAND_RUNTIME_HEAD", "NORTH_OFFLOAD_SLOT", "HEAD_NORTH_TREE", "DEPBOX", "NORTH_WORKER_SANDBOXED"] {
        unconfined.env_remove(name);
    }
    let ran = unconfined.output().unwrap();
    let said = String::from_utf8_lossy(&ran.stdout);
    assert!(ran.status.success() && said.contains("live north") && !said.contains("planted"), "an unconfined head-north of another tree ran the north a confined check planted for it: {said} {}", String::from_utf8_lossy(&ran.stderr));
}
