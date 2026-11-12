use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use north_base::platform::land_queue;

use super::{Hand, Waiting};
use crate::land::enqueue::Queued;
use crate::land::queue::{self, Ticket};
use crate::landing::lane_squash::Squash;

const TEST: &str = "land::enqueue::verdict::tests::a_wait_with_no_verdict_for_60_seconds_asks_the_lander_and_says_what_it_waits_on";
const SCRATCH: &str = "NORTH_VERDICT_TEST_SCRATCH";
const NAME: &str = "1-lane.json";

fn asked_after(scratch: &Path, quiet_secs: u64) -> bool {
    let state_home = scratch.join(format!("state-{quiet_secs}"));
    let ticket = Ticket { origin: "origin".into(), branch: "lane".into(), head: "abc123".into(), ..Ticket::default() };
    let queued = Queued { path: PathBuf::from(NAME), outbox: true, ticket, squash: Squash::Unchanged };
    let hand = Hand { queued: &queued, state_home: &state_home, git_dir: scratch.to_owned(), destination: "main", lander: "lander-host" };
    let here = queue::directory(&state_home, "origin");
    let mut waiting = Waiting::new(NAME, here.clone(), Vec::new());
    hand.ask_when_quiet(&mut waiting, Duration::from_secs(quiet_secs));
    here.join(land_queue::OUTBOX).join(queue::ASKS).join(NAME).is_file() && waiting.ask.is_some()
}

#[test]
fn a_wait_with_no_verdict_for_60_seconds_asks_the_lander_and_says_what_it_waits_on() {
    if let Ok(scratch) = std::env::var(SCRATCH) {
        assert!(!asked_after(Path::new(&scratch), 59), "a wait quiet for 59 s must not ask yet");
        assert!(asked_after(Path::new(&scratch), 61), "a wait quiet for 61 s must write its ask");
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap()).args(["--exact", TEST, "--nocapture"]).env(SCRATCH, scratch.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    let asking: Vec<&str> = stdout.lines().filter(|line| line.contains("asking the lander")).collect();
    assert_eq!(asking.len(), 1, "{stdout}");
    assert!(asking[0].starts_with("safe-push: no verdict on lane (abc123) after ") && asking[0].ends_with("; asking the lander on lander-host"), "{}", asking[0]);
}
