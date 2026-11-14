use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use north_base::platform::wait;
use north_lander::land::heartbeat::Heartbeat;
use north_lander::land::jobs::Chores;
use north_lander::land::lander::Lander;
use north_lander::land::queue::{self, HandIn, Ticket};

use crate::fleet_launch::{environment, fixture};

const WATCHED_QUEUE: &str = "NORTH_IT_REFUSAL_WAKE_QUEUE";
const READY: &str = "NORTH_IT_REFUSAL_WAKE_READY";
const ORIGIN: &str = "https://example.invalid/north";
const BRIEF: &str = "Item: north#1194 finish at acceptance\nSurface: rust\n";
const QUEUED_MICROS: u128 = 1_794_000_000_000_000;
const REFUSAL: &str = "conflict with main at refusal.txt; rebase onto origin/main and hand in again";

fn north(root: &Path, args: &[&str]) -> Output {
    environment(Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")), root, "here").env("NORTH_TEST_ROOT", root).args(args).output().unwrap()
}

fn shown(root: &Path) -> String {
    String::from_utf8_lossy(&north(root, &["fleet", "show", "narrow"]).stdout).into_owned()
}

fn handed_in(queue_dir: &Path, root: &Path, branch: &str, micros: u128) -> (PathBuf, Ticket) {
    let path = queue_dir.join(queue::file_name(micros, branch));
    let hand_in = HandIn { wakes_session: true, ..HandIn::default() };
    let ticket = Ticket { origin: ORIGIN.into(), root: root.to_owned(), host: "here".into(), branch: branch.into(), head: "1".repeat(40), session: Some("narrow".into()), queued_at: "2026-11-12T04:00:00Z".into(), queued_micros: Some(micros), hand_in: Some(hand_in), ..Ticket::default() };
    queue::write(&path, &ticket).unwrap();
    north_base::land_ticket::queued(&path, "enqueued").unwrap();
    (path, ticket)
}

#[test]
fn verdict_watch() {
    let (Ok(queue_dir), Ok(ready)) = (std::env::var(WATCHED_QUEUE), std::env::var(READY)) else { return };
    let repo = PathBuf::from(&queue_dir).join("repo");
    let lander = Lander { repo: repo.clone(), worktree: repo, offload_owner: String::new(), queue: PathBuf::from(queue_dir), checker: Vec::new(), app_token: Vec::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default(), wake: std::sync::Arc::default() };
    let _watching = north_lander::land::serve::verdicts::watch(&lander);
    fs::write(&ready, "").unwrap();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
}

// KEEP(bug): a finish call returns at the queue, so a refusal reached its session only on the fleet tick's resume, tens of seconds late, and a landing spent a turn on a session with nothing left to do (north#1194).
#[test]
fn a_session_whose_finish_call_returned_at_the_queue_is_kept_past_its_report_and_only_a_refusal_starts_its_next_turn_from_the_lander_alone() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "report");
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let launched = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--system-prompt", brief.to_str().unwrap(), "start the box"]);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    assert!(wait::until(Duration::from_secs(20), || shown(root).contains("Done: fake report")), "the session never made its closing report: {}", shown(root));

    let queue_dir = queue::directory(&root.join(".local/state"), ORIGIN);
    fs::create_dir_all(&queue_dir).unwrap();
    let (refused, refused_ticket) = handed_in(&queue_dir, root, "refused-lane", QUEUED_MICROS);
    let (landed, landed_ticket) = handed_in(&queue_dir, root, "landed-lane", QUEUED_MICROS + 1);
    let reaped = north(root, &["fleet", "reap", "--dry-run"]);
    assert!(reaped.status.success() && !String::from_utf8_lossy(&reaped.stdout).contains("retired narrow"), "a session whose ticket is undecided was retired on its closing report: {}", String::from_utf8_lossy(&reaped.stdout));

    let ready = root.join("verdicts.ready");
    let mut watcher = environment(Command::new(std::env::current_exe().unwrap()), root, "here");
    watcher.args(["--exact", "refusal_wake::verdict_watch", "--nocapture", "--test-threads", "1"]).env(WATCHED_QUEUE, &queue_dir).env(READY, &ready).env("NORTH_LAND_NORTH", crate::support::bin_exe!("CARGO_BIN_EXE_north")).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let watcher = watcher.spawn().unwrap();
    assert!(wait::until(Duration::from_secs(20), || ready.exists()), "the lander never started watching its verdicts");

    queue::decide(&landed, landed_ticket, "landed 2222222222222222222222222222222222222222 on main".to_owned()).unwrap();
    queue::decide(&refused, refused_ticket, REFUSAL.to_owned()).unwrap();
    let woken = wait::until(Duration::from_secs(20), || shown(root).split_once(&format!("refused-lane: {REFUSAL}")).is_some_and(|(_, after)| after.contains("turn: success")));
    let transcript = shown(root);
    let mut watcher = watcher;
    let _ = watcher.kill();
    let told = watcher.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&told.stdout).into_owned();
    north(root, &["fleet", "forget", "narrow"]);

    assert!(woken, "the refusal started no turn in the session:\n{transcript}\n{said}");
    assert_eq!(transcript.matches("[north: the lander has decided]").count(), 1, "the session was woken other than once, by the refusal alone:\n{transcript}\n{said}");
    assert!(!transcript.contains("landed-lane"), "a landing woke the session:\n{transcript}\n{said}");
}
