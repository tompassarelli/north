use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::*;

const PATIENCE: Duration = Duration::from_secs(10);

fn fake_proxy(directory: &Path, script: &str) {
    let program = directory.join("xdg-dbus-proxy");
    fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
}

fn shell(script: &str) -> Vec<String> {
    ["sh", "-c", script].map(String::from).to_vec()
}

#[tokio::test]
async fn the_child_sees_the_socket_as_its_session_bus_and_the_proxy_dies_after_it() {
    let scratch = tempfile::tempdir().unwrap();
    let bin = scratch.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fake_proxy(&bin, r#"echo $$ > "$2.pid"; : > "$2"; exec sleep 60"#);
    let socket = scratch.path().join("secrets.bus");
    let seen = scratch.path().join("seen");
    let script = format!(r#"printf %s "$DBUS_SESSION_BUS_ADDRESS" > {}; exit 7"#, seen.display());

    let status = exec(&socket, &shell(&script), bin.as_os_str(), PATIENCE, quiet()).await.unwrap();

    assert_eq!(exit_code(status), 7);
    assert_eq!(fs::read_to_string(&seen).unwrap(), format!("unix:path={}", socket.display()));
    let proxy = fs::read_to_string(scratch.path().join("secrets.bus.pid")).unwrap();
    let proxy = Pid::from_raw(proxy.trim().parse().unwrap()).unwrap();
    assert!(rustix::process::test_kill_process(proxy).is_err(), "the proxy outlived the child");
    assert!(!socket.exists());
}

#[tokio::test]
async fn a_proxy_that_exits_before_opening_its_socket_fails_without_running_the_child() {
    let scratch = tempfile::tempdir().unwrap();
    let bin = scratch.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fake_proxy(&bin, "exit 1");
    let ran = scratch.path().join("ran");
    let script = format!("touch {}", ran.display());

    let error = exec(&scratch.path().join("secrets.bus"), &shell(&script), bin.as_os_str(), PATIENCE, quiet()).await.unwrap_err();

    assert!(error.to_string().contains("exited"));
    assert!(!ran.exists());
}

#[test]
fn arguments_need_a_socket_a_separator_and_a_command() {
    let words = |text: &[&str]| text.iter().map(|word| word.to_string()).collect::<Vec<_>>();
    assert!(parse(&words(&["sock", "--", "chrome", "--flag"])).is_some());
    assert!(parse(&words(&["sock", "--"])).is_none());
    assert!(parse(&words(&["sock", "chrome"])).is_none());
}

fn quiet() -> mpsc::UnboundedReceiver<Signal> {
    mpsc::unbounded_channel().1
}

#[tokio::test]
async fn a_stop_signal_during_the_socket_wait_ends_the_run_without_running_the_child() {
    let scratch = tempfile::tempdir().unwrap();
    let bin = scratch.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fake_proxy(&bin, r#"echo $$ > "$2.pid"; exec sleep 60"#);
    let ran = scratch.path().join("ran");
    let script = format!("touch {}", ran.display());
    let (sender, signals) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        sender.send(Signal::Term).unwrap();
    });

    let error = exec(&scratch.path().join("secrets.bus"), &shell(&script), bin.as_os_str(), PATIENCE, signals).await.unwrap_err();

    assert!(error.to_string().contains("stopped"));
    assert!(!ran.exists());
    let proxy = fs::read_to_string(scratch.path().join("secrets.bus.pid")).unwrap();
    assert!(rustix::process::test_kill_process(Pid::from_raw(proxy.trim().parse().unwrap()).unwrap()).is_err());
}

#[tokio::test]
async fn a_stop_signal_reaches_the_running_child() {
    let scratch = tempfile::tempdir().unwrap();
    let bin = scratch.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fake_proxy(&bin, r#": > "$2"; exec sleep 60"#);
    let (sender, signals) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        sender.send(Signal::Term).unwrap();
    });

    let status = exec(&scratch.path().join("secrets.bus"), &shell("exec sleep 60"), bin.as_os_str(), PATIENCE, signals).await.unwrap();

    assert_eq!(exit_code(status), 128 + 15);
}
