use north_core::foundation::shim;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn the_link_exits_when_its_relay_ends_though_the_host_holds_its_input_open() {
    let scratch = tempfile::tempdir().unwrap();
    let bin = scratch.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    shim::write_script(&bin.join("ssh"), "echo gone\n");
    let mut link = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["session-link", "--ssh", "tom@far", "--unit", "north-fleet-box.service", "--dir", "/held/box", "--nonce", "n", "--launch", "true"])
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _held_input = link.stdin.take();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = link.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            link.kill().unwrap();
            panic!("north session-link outlived its relay while its input stayed open");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(1), "a far side holding another launch ends the link with 1");
}
