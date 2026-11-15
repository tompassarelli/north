use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use crate::fleet_launch::{environment, fixture};

const BRIEF: &str = "Item: north#1604 a fleet message during a hand-in\nSurface: rust\n";
const HAND_IN: &str = "safe-push --to main --wait";
const MESSAGE: &str = "a fleet message sent while the hand-in runs";

fn north(root: &Path, args: &[&str]) -> Output {
    environment(Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")), root, "here").args(args).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_fleet_message_does_not_end_a_running_hand_in() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "hand-in");
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    let launched = environment(Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")), root, "here")
        .env("NORTH_TEST_ROOT", root)
        .args(["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--system-prompt", &brief, "start the box"])
        .output()
        .unwrap();
    let mut shown = String::new();
    let running = north_core::platform::wait::until(Duration::from_secs(30), || {
        shown = text(&north(root, &["fleet", "show", "narrow"]).stdout);
        shown.contains(HAND_IN)
    });
    let sent = north(root, &["fleet", "send", "--from", "service:fleet-test", "--now", "narrow", MESSAGE]);
    let interrupts = fs::read_to_string(root.join("claude-interrupts")).unwrap_or_default();
    north(root, &["fleet", "forget", "narrow"]);

    assert!(launched.status.success(), "{}", text(&launched.stderr));
    assert!(running, "the hand-in never started: {shown}");
    assert!(sent.status.success(), "{}", text(&sent.stderr));
    assert!(interrupts.is_empty(), "the fleet message interrupted the running hand-in: {interrupts:?}");
}
